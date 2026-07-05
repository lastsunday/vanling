use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use framework::id::gen_id;
use futures_util::future::{join, join_all};
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};
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
use service::chobits::message::{
    hello::HelloMessage,
    listen::{ListenMessage, ListenMode, ListenState},
    tts::TtsState,
};
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{error, info};

use crate::{
    asr::AsrFactory,
    config::{
        audio::AudioConfig, matrix::MatrixConfig, mcp::McpConfig, session::SessionConfig,
        vad::VadConfig,
    },
    llm::LlmFactory,
    mcp::{
        client::server::ServerMcpClient,
        mcp_host::{McpHost, UnionMcpHost},
    },
    vad::VadFactory,
    ws::session::{SessionBuilder, listener::DefaultListener},
};
use service::ws::frame::{Frame, FrameResult};

pub async fn start(
    matrix_config: Arc<MatrixConfig>,
    session_config: Arc<SessionConfig>,
    mcp_config: Arc<McpConfig>,
    vad_config: Arc<VadConfig>,
    audio_config: Arc<AudioConfig>,
) -> Result<(), Box<dyn Error>> {
    let bot = Bot::build(
        matrix_config,
        session_config,
        mcp_config,
        vad_config,
        audio_config,
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
    mcp_config: Arc<McpConfig>,
    vad_config: Arc<VadConfig>,
    audio_config: Arc<AudioConfig>,
}

impl Bot {
    /// Build the `Bot` from the config.
    // TODO: session token save and reuse
    async fn build(
        matrix_config: Arc<MatrixConfig>,
        session_config: Arc<SessionConfig>,
        mcp_config: Arc<McpConfig>,
        vad_config: Arc<VadConfig>,
        audio_config: Arc<AudioConfig>,
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
            mcp_config,
            vad_config,
            audio_config,
            user_id,
            session_map: Arc::new(Mutex::new(HashMap::new())),
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
            Some(Duration::from_secs(30)),
        ));

        info!("matrix client listening...");
        while let Some(response) = sync_stream.try_next().await? {
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
            let mut mcp_host = UnionMcpHost::new(Some(id.clone()));
            let uri_list = self.mcp_config.uri_list.as_ref();
            if let Some(uri_list) = uri_list {
                for uri in uri_list {
                    let server_mcp_client = create_server_mcp_client(uri.to_string()).await;
                    match server_mcp_client {
                        Ok(server_mcp_client) => {
                            mcp_host.add_client(Box::new(server_mcp_client)).await;
                        }
                        Err(e) => {
                            error!("{:?}", e);
                        }
                    }
                }
            }
            let (session, input_tx, output_rx) = SessionBuilder::new()
                .with_id(id.clone())
                .with_listener(Box::new(DefaultListener::new(
                    VadFactory::create_model(&self.vad_config),
                    AsrFactory::global().default().clone(),
                    self.audio_config.clone(),
                )))
                .with_model(LlmFactory::global().default())
                .with_mcp_host(Arc::new(Mutex::new(mcp_host)))
                .with_config(self.session_config.clone())
                .with_audio_config(self.audio_config.clone())
                .build();
            tokio::spawn(session.start());
            let mut output = UnboundedReceiverStream::new(output_rx);
            // send hello frame
            input_tx.send(Frame::Hello(HelloMessage {
                ..Default::default()
            }))?;
            if let Some(data) = output.next().await {
                match data.payload {
                    Ok(frame_result) => {
                        if let FrameResult::HelloResult(HelloMessage {
                            message: _,
                            version: _,
                            transport: _,
                            audio_params: _,
                            features: _,
                            session_id: _,
                        }) = frame_result
                        {
                            // TODO: handle hello result
                        } else {
                            return Err(anyhow::anyhow!(format!(
                                "not recv hello frame result,frame result = {:?}",
                                frame_result
                            ))
                            .into());
                        }
                    }
                    Err(e) => return Err(anyhow::anyhow!(e.to_string()).into()),
                }
            }
            //start frame listener async task
            let matrix_client = self.matrix_client.clone();
            let room_id_clone = room_id.clone();
            tokio::spawn(async move {
                let id = room_id_clone;
                while let Some(data) = output.next().await {
                    match data.payload {
                        Ok(frame_result) => match frame_result {
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
                                                        let _ =
                                                            matrix_client.send_request(req).await;
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
                            FrameResult::McpResult(_mcp_request) => todo!(),
                        },
                        Err(_e) => {
                            // TODO: handle frame error
                        }
                    }
                }
            });
            // TODO: add to session map
            session_map.insert(session_key.to_string(), input_tx);
        }
        let tx = session_map
            .get(session_key)
            .unwrap_or_else(|| panic!("session not exists for provided session key"))
            .clone();
        drop(session_map);
        let _ = tx.send(Frame::Listen(ListenMessage {
            state: ListenState::Detect,
            mmod: Some(ListenMode::Manual),
            text: Some(t.body),
            ..Default::default()
        }));
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

async fn create_server_mcp_client(uri: String) -> anyhow::Result<ServerMcpClient> {
    let config = StreamableHttpClientTransportConfig::with_uri(uri);
    let transport = StreamableHttpClientTransport::from_config(config);
    let mut server_mcp_client = ServerMcpClient::new(transport).await?;
    server_mcp_client.init().await?;
    Ok(server_mcp_client)
}
