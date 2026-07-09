use std::sync::Arc;

use api::record::recorder::{Dir, EntryKind, FrameDetail, Recorder};
use api::ws::filter::{FilterCtx, OutputFilter, RecorderOutputFilter};
use framework::error::AppError;
use framework::error::critical_code::CriticalErrorCode;
use rmcp::model::{JsonRpcRequest, Request, RequestId};
use service::chobits::frame::{FrameResult, OutputMessage};
use service::chobits::message::audio::AudioMessage;
use service::chobits::message::hello::HelloMessage;
use service::chobits::message::llm::LlmMessage;
use service::chobits::message::mcp::McpRequest;
use service::chobits::message::stt::SttMessage;
use service::chobits::message::tts::{TtsMessage, TtsState};

fn make_msg(round_id: Option<&str>, payload: FrameResult) -> OutputMessage {
    OutputMessage {
        epoch: 0,
        round_id: round_id.map(String::from),
        session_id: "test-session".into(),
        payload,
    }
}

async fn make_recorder() -> Arc<Recorder> {
    let conn = framework::database::establish_connection("sqlite::memory:")
        .await
        .expect("create memory db");
    Arc::new(Recorder::new(conn))
}

async fn process(filter: &RecorderOutputFilter, msg: OutputMessage) {
    let ctx = FilterCtx {
        session_id: "test-session".into(),
    };
    let _ = OutputFilter::process(filter, &ctx, msg).await;
}

// 1. recorder=None: all messages pass through without panic
#[tokio::test]
async fn test_recorder_none() {
    let filter = RecorderOutputFilter::new(None, "test".into());

    let msgs: Vec<OutputMessage> = vec![
        make_msg(
            Some("r1"),
            FrameResult::HelloResult(HelloMessage::default()),
        ),
        make_msg(Some("r1"), FrameResult::CloseResult),
        make_msg(
            Some("r1"),
            FrameResult::STTResult(SttMessage::new(None, None)),
        ),
        make_msg(None, FrameResult::HelloResult(HelloMessage::default())),
        make_msg(None, FrameResult::CloseResult),
        make_msg(None, FrameResult::STTResult(SttMessage::new(None, None))),
    ];
    for msg in msgs {
        let ctx = FilterCtx {
            session_id: "test".into(),
        };
        let _ = OutputFilter::process(&filter, &ctx, msg).await;
    }
}

// 2. round_id=None: HelloResult/CloseResult recorded; McpResult skipped
#[tokio::test]
async fn test_no_round_id_records_hello_close() {
    let recorder = make_recorder().await;
    let filter = RecorderOutputFilter::new(Some(recorder.clone()), "test".into());

    process(
        &filter,
        make_msg(None, FrameResult::HelloResult(HelloMessage::default())),
    )
    .await;
    process(&filter, make_msg(None, FrameResult::CloseResult)).await;

    let mcp_req = McpRequest::new(
        Some("test".into()),
        JsonRpcRequest::new(RequestId::Number(0), Request::new(serde_json::Map::new())),
    );
    process(&filter, make_msg(None, FrameResult::McpResult(mcp_req))).await;

    let entries = recorder.entries_snapshot();
    assert_eq!(entries.len(), 2, "only Hello and Close should be recorded");
    assert!(
        matches!(
            &entries[0].kind,
            EntryKind::Frame {
                detail: FrameDetail::Hello,
                ..
            }
        ),
        "first entry should be Hello"
    );
    assert!(
        matches!(
            &entries[1].kind,
            EntryKind::Frame {
                detail: FrameDetail::Close,
                ..
            }
        ),
        "second entry should be Close"
    );
}

// 3. full round: STT + LLM + Audio×2 + TTS(Stop); 5 entries; current_round_id cleared after Stop
#[tokio::test]
async fn test_full_round() {
    let recorder = make_recorder().await;
    let filter = RecorderOutputFilter::new(Some(recorder.clone()), "test".into());
    let rid = "round-1";

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::STTResult(SttMessage::new(None, Some("hello".into()))),
        ),
    )
    .await;

    let mut llm = LlmMessage::new(None, None, None);
    llm.full_text = Some("hello world".into());
    process(&filter, make_msg(Some(rid), FrameResult::LLMResult(llm))).await;

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::AudioResult(AudioMessage::new(None, vec![0u8; 10])),
        ),
    )
    .await;
    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::AudioResult(AudioMessage::new(None, vec![0u8; 10])),
        ),
    )
    .await;

    assert_eq!(recorder.entries_snapshot().len(), 5);

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::TTSResult(TtsMessage::new(None, Some(TtsState::Stop), None)),
        ),
    )
    .await;

    assert!(
        recorder.entries_snapshot().is_empty(),
        "entries should be taken by end_round"
    );
    assert!(!recorder.has_active_round(), "round should be ended");
}

// 4. round switch: old round auto-interrupted; both rounds' entries preserved
#[tokio::test]
async fn test_different_rounds() {
    let recorder = make_recorder().await;
    let filter = RecorderOutputFilter::new(Some(recorder.clone()), "test".into());

    process(
        &filter,
        make_msg(
            Some("A"),
            FrameResult::STTResult(SttMessage::new(None, Some("hi".into()))),
        ),
    )
    .await;
    process(
        &filter,
        make_msg(
            Some("A"),
            FrameResult::AudioResult(AudioMessage::new(None, vec![0u8; 10])),
        ),
    )
    .await;

    assert_eq!(
        recorder.entries_snapshot().len(),
        2,
        "round A should have 2 entries"
    );

    process(
        &filter,
        make_msg(
            Some("B"),
            FrameResult::STTResult(SttMessage::new(None, Some("hello".into()))),
        ),
    )
    .await;

    let entries = recorder.entries_snapshot();
    assert_eq!(
        entries.len(),
        1,
        "only round B's entry should remain in buffer"
    );
    assert!(
        matches!(
            &entries[0].kind,
            EntryKind::Frame {
                detail: FrameDetail::STTResult,
                ..
            }
        ),
        "the remaining entry should be STTResult"
    );
}

// 5. TTS(Stop) triggers end_round(Completed) and entries are taken
#[tokio::test]
async fn test_tts_stop_ends_round() {
    let recorder = make_recorder().await;
    let filter = RecorderOutputFilter::new(Some(recorder.clone()), "test".into());
    let rid = "r1";

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::STTResult(SttMessage::new(None, Some("hi".into()))),
        ),
    )
    .await;

    assert_eq!(recorder.entries_snapshot().len(), 1);

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::TTSResult(TtsMessage::new(None, Some(TtsState::Stop), None)),
        ),
    )
    .await;

    assert!(
        recorder.entries_snapshot().is_empty(),
        "entries should be taken by end_round"
    );
    assert!(!recorder.has_active_round(), "round should be completed");
}

// 6. Error output frames are recorded
#[tokio::test]
async fn test_error_output_recorded() {
    let recorder = make_recorder().await;
    let filter = RecorderOutputFilter::new(Some(recorder.clone()), "test".into());

    process(
        &filter,
        make_msg(
            Some("r1"),
            FrameResult::Error(AppError::from_code(CriticalErrorCode::InternalError)),
        ),
    )
    .await;

    let entries = recorder.entries_snapshot();
    assert_eq!(entries.len(), 1);
    assert!(
        matches!(
            &entries[0].kind,
            EntryKind::Frame {
                detail: FrameDetail::Error,
                dir: Dir::Output,
                ..
            }
        ),
        "Error output frame should be recorded"
    );
}

// 7. same round_id: all messages recorded, start_round fires only once
#[tokio::test]
async fn test_interrupt_duplicate_round_id() {
    let recorder = make_recorder().await;
    let filter = RecorderOutputFilter::new(Some(recorder.clone()), "test".into());
    let rid = "same-round";

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::STTResult(SttMessage::new(None, Some("a".into()))),
        ),
    )
    .await;
    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::LLMResult(LlmMessage::new(None, None, None)),
        ),
    )
    .await;
    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::AudioResult(AudioMessage::new(None, vec![0u8; 10])),
        ),
    )
    .await;

    assert_eq!(recorder.entries_snapshot().len(), 3);
    assert!(recorder.has_active_round(), "round should still be active");
}

// 8. TTSResult(SentenceStart) with text → TtsText + frame; without text → frame only
#[tokio::test]
async fn test_tts_non_stop_state() {
    let recorder = make_recorder().await;
    let filter = RecorderOutputFilter::new(Some(recorder.clone()), "test".into());
    let rid = "r1";

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::TTSResult(TtsMessage::new(
                None,
                Some(TtsState::SentenceStart),
                Some("hi".into()),
            )),
        ),
    )
    .await;

    let entries = recorder.entries_snapshot();
    assert_eq!(entries.len(), 2, "with text: TtsText + Frame expected");
    assert!(
        matches!(&entries[0].kind, EntryKind::TtsText { text } if text == "hi"),
        "first entry should be TtsText with correct text"
    );
    assert!(
        matches!(
            &entries[1].kind,
            EntryKind::Frame {
                detail: FrameDetail::TTSResult,
                ..
            }
        ),
        "second entry should be TTSResult frame"
    );

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::TTSResult(TtsMessage::new(None, Some(TtsState::Stop), None)),
        ),
    )
    .await;

    let recorder2 = make_recorder().await;
    let filter2 = RecorderOutputFilter::new(Some(recorder2.clone()), "test".into());
    process(
        &filter2,
        make_msg(
            Some("r2"),
            FrameResult::TTSResult(TtsMessage::new(None, Some(TtsState::Start), None)),
        ),
    )
    .await;

    let entries2 = recorder2.entries_snapshot();
    assert_eq!(entries2.len(), 1, "without text: only Frame expected");
    assert!(
        matches!(
            &entries2[0].kind,
            EntryKind::Frame {
                detail: FrameDetail::TTSResult,
                ..
            }
        ),
        "entry should be TTSResult frame"
    );
}

// 9. entry ordering matches input order
#[tokio::test]
async fn test_entry_ordering() {
    let recorder = make_recorder().await;
    let filter = RecorderOutputFilter::new(Some(recorder.clone()), "test".into());
    let rid = "order-round";

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::STTResult(SttMessage::new(None, Some("first".into()))),
        ),
    )
    .await;

    let mut llm = LlmMessage::new(None, None, None);
    llm.full_text = Some("second".into());
    process(&filter, make_msg(Some(rid), FrameResult::LLMResult(llm))).await;

    process(
        &filter,
        make_msg(
            Some(rid),
            FrameResult::AudioResult(AudioMessage::new(None, vec![0u8; 10])),
        ),
    )
    .await;

    let entries = recorder.entries_snapshot();
    assert_eq!(
        entries.len(),
        4,
        "STTResult frame + LlmText + LLMResult frame + AudioResult frame"
    );

    assert!(
        matches!(
            &entries[0].kind,
            EntryKind::Frame {
                detail: FrameDetail::STTResult,
                ..
            }
        ),
        "entry[0] should be STTResult"
    );
    assert!(
        matches!(&entries[1].kind, EntryKind::LlmText { .. }),
        "entry[1] should be LlmText"
    );
    assert!(
        matches!(
            &entries[2].kind,
            EntryKind::Frame {
                detail: FrameDetail::LLMResult,
                ..
            }
        ),
        "entry[2] should be LLMResult frame"
    );
    assert!(
        matches!(
            &entries[3].kind,
            EntryKind::Frame {
                detail: FrameDetail::AudioResult,
                ..
            }
        ),
        "entry[3] should be AudioResult"
    );
}
