// use api::{
//     asr::AsrFactory,
//     config::{
//         AsrModel, LlmModel, TtsModel, VadModel, asr::AsrConfig, audio::AudioConfig, llm::LlmConfig,
//         session::SessionConfig, tts::TtsConfig, vad::VadConfig,
//     },
//     llm::LlmFactory,
//     mcp::mcp_host::UnionMcpHost,
//     record::{collector::RecordCollector, observer::SessionObserver},
//     tts::TtsFactory,
//     vad::VadFactory,
//     ws::frame::{Frame, FrameResult},
//     ws::session::{SessionBuilder, listener::DefaultListener},
// };
// use entity::{prelude::*, round_data};
// use framework::id::gen_id;
// use sea_orm::entity::prelude::*;
// use service::chobits::message::{
//     hello::HelloMessage,
//     tts::TtsState,
// };
// use std::{sync::Arc, time::Duration};
//
// use tokio::sync::Mutex;
// use tokio_stream::StreamExt;
// use tracing_test::traced_test;
//
// use crate::common::{setup_database, tear_down};
//
// // TODO: failed - "record data not fully flushed to DB within 5s" (DB flush timeout)
// #[tokio::test]
// #[traced_test]
// /// Test that record data is persisted to DB after a full round
// /// Using Echo LLM + Mute TTS + Void ASR + Earshot VAD
// async fn test_full_flow() -> anyhow::Result<()> {
//     let (container, state) = setup_database().await;
//     let record = Arc::new(RecordCollector::new(state.conn.clone())) as Arc<dyn SessionObserver>;
//
//     let audio_config = Arc::new(AudioConfig {
//         input_sample_rate: Some(16000),
//         input_frame_duration: Some(20_u64),
//         input_channel: Some(1),
//         output_sample_rate: Some(16000),
//         output_channel: Some(1),
//         output_frame_duration: Some(20_u64),
//     });
//     let session_id = gen_id();
//     let mut session = SessionBuilder::new()
//         .with_listener(Box::new(DefaultListener::new(
//             VadFactory::create_model(&Arc::new(
//                 VadConfig {
//                     model: Some(VadModel::Earshot),
//                     ..Default::default()
//                 },
//             )),
//             Arc::new(Mutex::new(AsrFactory::create_model(&AsrConfig {
//                 model: Some(AsrModel::Void),
//                 ..Default::default()
//             }))),
//             audio_config.clone(),
//         )))
//         .with_id(session_id.clone())
//         .with_model(Arc::new(LlmFactory::create_model(&LlmConfig {
//             model: Some(LlmModel::Echo),
//             ..Default::default()
//         })))
//         .with_tts(Arc::new(
//             TtsFactory::create_model(
//                 &TtsConfig {
//                     model: Some(TtsModel::Mute),
//                     ..Default::default()
//                 },
//                 &audio_config,
//             )
//             .await
//             .unwrap(),
//         ))
//         .with_mcp_host(Arc::new(Mutex::new(UnionMcpHost::new(Some(
//             session_id.clone(),
//         )))))
//         .with_config(Arc::new(SessionConfig {
//             close_connection_no_voice_time: Some(3000),
//             silence_voice_timeout: Some(1200),
//             system_prompt: Some(String::from(
//                 "你是一个助手，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等。",
//             )),
//             max_prompt_len: Some(3000),
//         }))
//         .with_audio_config(audio_config.clone())
//         .add_observer(record)
//         .build();
//
//     for observer in &session.observers {
//         observer.on_session_start(&session_id).await;
//     }
//
//     session.start().await?;
//     let (mut output, _, _, _, _) = session.output_frame().await;
//
//     // Hello
//     session
//         .accept_frame(&Frame::Hello(HelloMessage {
//             ..Default::default()
//         }))
//         .await;
//     assert!(matches!(
//         output.next().await.unwrap().payload.unwrap(),
//         FrameResult::HelloResult(..)
//     ));
//
//     // Send text message via Input
//     session
//         .accept_frame(&Frame::Input { text: "Hello".to_string() })
//         .await;
//
//     // Consume output until TTS::Stop
//     loop {
//         let frame = output.next().await.unwrap().payload.unwrap();
//         if let FrameResult::TTSResult(msg) = &frame
//             && msg.state == Some(TtsState::Stop)
//         {
//             break;
//         }
//     }
//
//     session.stop().await;
//
//     // Poll DB for record data (wait until llm round_data is flushed)
//     // Mute TTS doesn't produce raw_pcm, so on_tts_delta is never called
//     // Wait for at least the llm entry — flush_to_db inserts sequentially
//     // (text row first, then llm row), so checking data.len() >= 1 is racy.
//     let conn = &state.conn;
//     let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
//     let (rounds, data) = loop {
//         let rounds = Round::find().all(conn).await?;
//         if let Some(round) = rounds.first() {
//             let data = round_data::Entity::find()
//                 .filter(round_data::Column::RoundId.eq(&round.id))
//                 .all(conn)
//                 .await?;
//             if data.iter().any(|d| d.data_type == "llm") {
//                 break (rounds, data);
//             }
//         }
//         tokio::time::sleep(Duration::from_millis(20)).await;
//         anyhow::ensure!(
//             tokio::time::Instant::now() < deadline,
//             "record data not fully flushed to DB within 5s"
//         );
//     };
//
//     assert_eq!(rounds.len(), 1, "expected 1 round");
//     assert_eq!(
//         rounds[0].session_id, session_id,
//         "round.session_id should match session_id"
//     );
//
//     let llm = data
//         .iter()
//         .find(|d| d.data_type == "llm")
//         .expect("llm round_data not found");
//     assert_eq!(llm.text, Some("Hello".to_string()));
//     assert!(llm.data.is_none());
//
//     let _ = &state.conn.close().await?;
//     tear_down(container).await;
//
//     Ok(())
// }
