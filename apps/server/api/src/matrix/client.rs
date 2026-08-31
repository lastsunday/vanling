use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

const MATRIX_SYNC_TIMEOUT: Duration = Duration::from_secs(30);

use framework::id::gen_id;
use futures_util::future::{join, join_all};
use ruma::{
    OwnedRoomId, OwnedUserId, TransactionId, UserId,
    api::client::{
        filter::FilterDefinition, membership::join_room_by_id, message::send_message_event,
        sync::sync_events,
    },
    assign,
    events::{
        AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
        room::message::{MessageType, RoomMessageEventContent},
    },
    presence::PresenceState,
    serde::Raw,
};
use service::component::mcp::McpRegistry;
use service::frame::{Frame, FrameResult, InputMode};
use service::message::{hello::HelloMessage, tts::TtsState};
use service::pipeline::{AsrNode, LingNode, OpusDecodeNode, TtsNode, TurnNode, VadNode};
use service::session::SessionConfig as ServiceSessionConfig;
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::{
    component::asr::AsrManager,
    component::ling::LingCoreBuilder,
    component::llm::LlmManager,
    component::mcp::client::{create_external_mcp_client, resolve_mcp_auth_token},
    component::tts::TtsManager,
    component::vad::pool::VadPool,
    config::{
        ling::LingConfig, matrix::MatrixConfig, mcp::McpConfig, session::SessionConfig,
        vad::VadConfig,
    },
};

pub async fn start(
    matrix_config: Arc<MatrixConfig>,
    session_config: Arc<SessionConfig>,
    ling_config: Arc<LingConfig>,
    mcp_config: Arc<McpConfig>,
    vad_config: Arc<VadConfig>,
    shutdown_token: CancellationToken,
) -> Result<(), Box<dyn Error>> {
    let bot = Bot::build(
        matrix_config,
        session_config,
        ling_config,
        mcp_config,
        vad_config,
        shutdown_token,
    )
    .await?;
    bot.run().await?;
    Ok(())
}

type HttpClient = ruma_client::http_client::Reqwest;
type MatrixClient = ruma_client::Client<HttpClient>;

/// The bot.
struct Bot {
    /// The client to use to make requests against the Matrix API.
    matrix_client: MatrixClient,
    /// The user ID of the Matrix account used by the bot.
    user_id: OwnedUserId,
    session_map: Arc<Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<Frame>>>>,
    session_config: Arc<SessionConfig>,
    ling_config: Arc<LingConfig>,
    mcp_config: Arc<McpConfig>,
    vad_config: Arc<VadConfig>,
    shutdown: CancellationToken,
}

impl Bot {
    /// Build the `Bot` from the config.
    // TODO: session token save and reuse
    async fn build(
        matrix_config: Arc<MatrixConfig>,
        session_config: Arc<SessionConfig>,
        ling_config: Arc<LingConfig>,
        mcp_config: Arc<McpConfig>,
        vad_config: Arc<VadConfig>,
        shutdown_token: CancellationToken,
    ) -> Result<Self, Box<dyn Error>> {
        let matrix_client = ruma_client::Client::builder()
            .homeserver_url(
                matrix_config
                    .homeserver
                    .clone()
                    .expect("matrix homeserver is empty"),
            )
            .build::<HttpClient>()
            .await?;
        let username = matrix_config
            .client_username
            .clone()
            .expect("matrix client username is empty");
        matrix_client
            .log_in(
                &username,
                &matrix_config
                    .client_password
                    .clone()
                    .expect("matrix client password is empty"),
                None,
                matrix_config.client_name.as_deref(),
            )
            .await?;
        let user_id = UserId::parse(username).expect("invalid matrix user id");
        Ok(Self {
            matrix_client,
            session_config,
            ling_config,
            mcp_config,
            vad_config,
            user_id,
            session_map: Arc::new(Mutex::new(HashMap::new())),
            shutdown: shutdown_token,
        })
    }

    /// Run the bot.
    async fn run(&self) -> Result<(), Box<dyn Error>> {
        // Perform an initial sync to ignore messages before the bot was launched.
        let filter = FilterDefinition::ignore_all().into();
        let initial_sync_response = self
            .matrix_client
            .send_request(assign!(sync_events::v3::Request::new(), {
                filter: Some(filter),
            }))
            .await?;

        // Ignore events from our bot.
        let not_senders = vec![self.user_id.clone()];
        let filter = {
            let mut filter = FilterDefinition::empty();
            filter.room.timeline.not_senders = not_senders;
            filter
        }
        .into();

        // Launch a sync loop to listen to messages and invites.
        let mut sync_stream = Box::pin(self.matrix_client.sync(
            Some(filter),
            initial_sync_response.next_batch,
            PresenceState::Online,
            Some(MATRIX_SYNC_TIMEOUT),
        ));

        info!("matrix client listening...");
        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => {
                    info!("matrix client shutting down");
                    break;
                }
                response = sync_stream.try_next() => {
                    let Some(response) = response? else { break };
                    let message_futures =
                        response
                            .rooms
                            .join
                            .iter()
                            .map(|(room_id, room_info)| async move {
                                // Use a regular for loop for the messages within one room to handle them sequentially
                                for e in &room_info.timeline.events {
                                    if let Err(err) = self.handle_message(e, room_id.to_owned()).await {
                                        error!("failed to respond to message: {err}");
                                    }
                                }
                            });

                    let invite_futures = response.rooms.invite.into_keys().map(|room_id| async move {
                        if let Err(err) = self.handle_invitations(room_id.clone()).await {
                            error!("failed to accept invitation for room {room_id}: {err}");
                        }
                    });

                    // Handle messages from different rooms as well as invites concurrently
                    join(join_all(message_futures), join_all(invite_futures)).await;
                }
            }
        }

        Ok(())
    }

    /// Handle the given message from the given room.
    async fn handle_message(
        &self,
        ev: &Raw<AnySyncTimelineEvent>,
        room_id: OwnedRoomId,
    ) -> Result<(), Box<dyn Error>> {
        // We are only interested in text messages that contain the word "joke".
        let Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
            SyncMessageLikeEvent::Original(m),
        ))) = ev.deserialize()
        else {
            return Ok(());
        };
        let MessageType::Text(t) = m.content.msgtype else {
            return Ok(());
        };

        info!("{}:\t{}", m.sender, t.body);
        // create session
        let session_key = &room_id.to_string();
        let session_map = self.session_map.clone();
        let mut session_map = session_map.lock().await;
        if !session_map.contains_key(session_key) {
            // init session
            let id = gen_id();
            let mcp_registry = Arc::new(Mutex::new(McpRegistry::new(Some(id.clone()))));
            if !self.mcp_config.server_list.is_empty() {
                for server in &self.mcp_config.server_list {
                    let external_mcp_client = create_external_mcp_client(
                        server.uri.clone(),
                        resolve_mcp_auth_token(server, &id),
                    )
                    .await;
                    match external_mcp_client {
                        Ok(server_mcp_client) => {
                            mcp_registry
                                .lock()
                                .await
                                .add_client(Arc::new(server_mcp_client))
                                .await;
                        }
                        Err(e) => {
                            error!("{:?}", e);
                        }
                    }
                }
            }

            let ling: Arc<dyn service::ling::Ling> = Arc::new(
                LingCoreBuilder::new()
                    .with_session_id(Some(id.clone()))
                    .with_model(LlmManager::global().default())
                    .with_mcp_registry(mcp_registry)
                    .with_preamble(self.ling_config.system_prompt.clone())
                    .build(),
            );

            let tts: Arc<dyn service::component::tts::Tts> = TtsManager::global().default();

            let session_config = ServiceSessionConfig {
                silence_voice_timeout: self.session_config.silence_voice_timeout,
                close_connection_no_activity_time: self
                    .session_config
                    .close_connection_no_activity_time,
                barge_in_lockout_ms: self.session_config.barge_in_lockout_ms,
            };

            let templates: Vec<Arc<dyn service::pipeline::Node>> = vec![
                Arc::new(OpusDecodeNode::new()) as Arc<dyn service::pipeline::Node>,
                Arc::new(VadNode::new(Arc::new(VadPool::new(
                    self.vad_config.clone(),
                )))) as Arc<dyn service::pipeline::Node>,
                Arc::new(AsrNode::new(AsrManager::global().default().clone()))
                    as Arc<dyn service::pipeline::Node>,
                Arc::new(TurnNode::new()) as Arc<dyn service::pipeline::Node>,
                Arc::new(LingNode::new(ling)) as Arc<dyn service::pipeline::Node>,
                Arc::new(TtsNode::new(tts)) as Arc<dyn service::pipeline::Node>,
            ];

            let session_ctx = service::session::SessionBuilder::new()
                .with_id(id.clone())
                .with_node_templates(templates)
                .with_config(session_config)
                .build();
            tokio::spawn(session_ctx.session.start());
            let mut output = UnboundedReceiverStream::new(session_ctx.output_rx);
            // send hello frame
            session_ctx.input_tx.send(Frame::Hello(HelloMessage {
                ..Default::default()
            }))?;
            if let Some(data) = output.next().await {
                match data.payload {
                    FrameResult::HelloResult(HelloMessage {
                        message: _,
                        version: _,
                        transport: _,
                        audio_params: _,
                        features: _,
                        session_id: _,
                    }) => {
                        // TODO: handle hello result
                    }
                    frame_result => {
                        return Err(anyhow::anyhow!(format!(
                            "not recv hello frame result,frame result = {:?}",
                            frame_result
                        ))
                        .into());
                    }
                }
            }
            //start frame listener async task
            let matrix_client = self.matrix_client.clone();
            let room_id_clone = room_id.clone();
            tokio::spawn(async move {
                let id = room_id_clone;
                while let Some(data) = output.next().await {
                    match data.payload {
                        FrameResult::HelloResult(_hello_message) => todo!(),
                        FrameResult::STTResult(stt_message) => {
                            // TODO:
                            info!("{:?}", stt_message);
                        }
                        FrameResult::LLMResult(_llm_message) => {
                            // TODO:
                        }
                        FrameResult::TTSResult(tts_message) => {
                            match tts_message.state {
                                Some(state) => match state {
                                    TtsState::Start => {
                                        // TODO:
                                    }
                                    TtsState::SentenceStart => {
                                        // TODO:
                                        if let Some(text) = tts_message.text {
                                            let text_content =
                                                RoomMessageEventContent::notice_plain(text);
                                            let txn_id = TransactionId::new();
                                            let req = send_message_event::v3::Request::new(
                                                id.to_owned(),
                                                txn_id,
                                                &text_content,
                                            );
                                            match req {
                                                Ok(req) => {
                                                    // Do nothing if we can't send the message.
                                                    let _ = matrix_client.send_request(req).await;
                                                }
                                                Err(_) => todo!(),
                                            }
                                        } else {
                                            // TODO: text is none
                                        }
                                    }
                                    TtsState::SentenceEnd => {
                                        // TODO:
                                    }
                                    TtsState::Stop => {

                                        // TODO:
                                    }
                                },
                                None => {
                                    // TODO:
                                }
                            }
                        }
                        FrameResult::AudioResult(_audio_message) => {
                            // TODO:
                        }
                        FrameResult::CloseResult => {
                            // TODO: shutdown session and clear session map
                        }
                        FrameResult::McpResult(_mcp_request) => {
                            // Unreachable: MCP frames are routed through
                            // DeviceMcpTransport channel, not session output_rx.
                        }
                        _ => {}
                    }
                }
            });
            // TODO: add to session map
            session_map.insert(session_key.to_string(), session_ctx.input_tx);
        }
        let tx = session_map
            .get(session_key)
            .unwrap_or_else(|| panic!("session not exists for provided session key"))
            .clone();
        drop(session_map);
        let _ = tx.send(Frame::Input {
            text: t.body,
            mode: InputMode::Normal,
        });
        Ok(())
    }

    /// Handle an invitation to the given room.
    async fn handle_invitations(&self, room_id: OwnedRoomId) -> Result<(), Box<dyn Error>> {
        info!("invited to {room_id}");
        self.matrix_client
            .send_request(join_room_by_id::v3::Request::new(room_id.clone()))
            .await?;
        Ok(())
    }
}
