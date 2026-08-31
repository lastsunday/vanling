use std::pin::Pin;
use std::sync::Arc;

use framework::error::AppError;
use futures::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::message::hello::AudioParam;
use crate::types::EmptyKind;

mod nodes;
pub use nodes::{AsrNode, LingNode, OpusDecodeNode, TtsNode, TurnNode, VadNode};

/// 事件流别名：整条链流转统一事件，`Err` 沿 `TryStreamExt` 短路透传。
pub type EventStream =
    Pin<Box<dyn Stream<Item = Result<PipelineEvent, AppError>> + Send + 'static>>;

/// 统一事件类型 —— 所有节点共享，流过整条链。
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    // 感知
    /// Opus 原始音频，源自 Frame::Voice
    AudioFrame(Vec<u8>),
    /// Opus 经 OpusDecodeNode 解码后的 PCM（样本 + 采样率）
    PcmFrame(Vec<f32>, u32),
    /// 语音起始（VAD 上升沿）
    SpeechStarted,
    /// 语音结束（VAD 下降沿）
    SpeechEnded,
    PartialTranscript(String),
    /// 无有效输入：原始信号，由中枢判别后经 `Prompt` 驱动提示语。
    EmptyInput,
    /// 对话 Act：中枢注入的提示语指令（含语境与重试次数），由 Ling 消费。
    Prompt {
        kind: EmptyKind,
        count: u32,
    },
    /// 关卡：一轮识别完成（ASR 产出）
    TurnText {
        text: String,
        prob: f32,
    },
    /// 回合判定收尾（turn 节点产出）
    TurnComplete {
        text: String,
        prob: f32,
    },
    /// 重配节点（Opus 解码，换 AudioParam）
    Configure(AudioParam),
    /// 内部控制：请求本轮 ASR 立即 finish（静音超时 / transport stall / ListenStop）
    FinishTurn,
    /// 监听模式切换（ListenStart 时注入）：`streaming=true` 流式实时识别；
    /// `streaming=false` 按键录音——AsrNode 仅缓冲、不实时解码，`FinishTurn` 时一次性识别。
    ListenMode {
        streaming: bool,
    },
    // 表达
    TextChunk {
        text: String,
        emotion: Option<String>,
    },
    AudioOut {
        audio: Vec<Vec<u8>>,
        is_first: bool,
        is_last: bool,
    },
}

/// 广播时机标签：进入节点输入前 / 节点产出后。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapPoint {
    Before,
    After,
}

/// 广播载荷：事件 + 时机标签，发给 Round 观察者。
#[derive(Debug, Clone)]
pub struct Tapped {
    pub point: TapPoint,
    pub event: PipelineEvent,
}

/// 观察者广播发送端（Round 注入）；无界 mpsc，控制信号**绝不丢弃**（丢包会静默破坏 barge-in/回合推进）。
#[derive(Clone)]
pub struct EventSink {
    tx: tokio::sync::mpsc::UnboundedSender<Tapped>,
}

impl EventSink {
    /// 新建一条广播通道，返回 sink 与订阅端（Round 持有订阅端）。
    pub fn channel() -> (Self, tokio::sync::mpsc::UnboundedReceiver<Tapped>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Tapped>();
        (Self { tx }, rx)
    }

    /// 引擎自动广播；无接收者则丢弃。
    pub(crate) fn send(&self, tapped: Tapped) {
        let _ = self.tx.send(tapped);
    }
}

// 节点上下文：非 per-call 重建，Round 构链时注入。
pub struct NodeContext {
    pub cancel: CancellationToken,
    pub emit: EventSink,
    pub session_id: String,
}

impl NodeContext {
    /// 无观察者的上下文（单测 / 无外发场景）。
    pub fn new(cancel: CancellationToken) -> Self {
        let (emit, _rx) = EventSink::channel();
        Self {
            cancel,
            emit,
            session_id: String::new(),
        }
    }

    /// 带观察者入口的上下文（Round 构链时注入）。
    pub fn with_emit(cancel: CancellationToken, emit: EventSink, session_id: String) -> Self {
        Self {
            cancel,
            emit,
            session_id,
        }
    }
}

/// 下行音频输出能力（服务端握手期声明 + pacer 节奏）。当前唯一能力成员。
#[derive(Debug, Clone)]
pub struct AudioSpec {
    pub sample_rate: u32,
    pub channel: u32,
    pub frame_duration_ms: u64,
}

/// 会话级能力值：任何 `Send + Sync + 'static` 的具体类型即可被节点声明、
/// 由 Session 以 `downcast_ref::<T>()` 按类型 look up。直接复用 `Any` 的下转机制，
/// 避免自定义 vtable 的 `as_any` 在 `dyn` 动态分派下类型标识失真（rustc 1.95 行为）。
pub type NodeCapability = dyn std::any::Any + Send + Sync;

/// 节点资源释放策略（框架强制）：`Immediate` 在进程完成（流消费到 `None`）时释放；
/// `Deferred` 由 `NodeChain::finish` 在整轮结束时统一释放。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMode {
    Immediate,
    Deferred,
}

pub trait Node: Send + Sync {
    fn new_instance(&self) -> Arc<dyn Node>;

    fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream;

    /// 模板级下行配置：Session 在 hello 时把 `Configure` 事件转发给模板一次，更新模板自身状态；
    /// 后续每轮 `new_instance` 从中继承。默认空实现 = 不接受配置。
    fn on_configure(&self, _event: &PipelineEvent) {}

    /// 能力 look up：节点声明的会话级能力集合（空 = 无）。Session 按类型 `downcast_ref` 检索。
    fn capabilities(&self) -> Vec<Box<NodeCapability>> {
        Vec::new()
    }

    /// 释放时机（框架强制，默认 `Deferred` 整轮结束释放）。
    fn release_mode(&self) -> ReleaseMode {
        ReleaseMode::Deferred
    }

    /// 整轮起始钩子：由 `NodeChain::begin` 驱动（默认空实现）。
    fn on_acquire(&self) {}

    /// 整轮收尾钩子：`Immediate` 节点由 `with_observer` 在流末释放；
    /// `Deferred` 节点由 `NodeChain::finish` 在整轮结束释放（默认空实现）。
    fn on_release(&self) {}
}

fn with_broadcast(stream: EventStream, point: TapPoint, ctx: &NodeContext) -> EventStream {
    let emit = ctx.emit.clone();
    Box::pin(stream.map(move |r| {
        if let Ok(ev) = &r {
            emit.send(Tapped {
                point,
                event: ev.clone(),
            });
        }
        r
    }))
}

pub fn compose(a: Arc<dyn Node>, b: Arc<dyn Node>) -> Arc<dyn Node> {
    struct Composite {
        a: Arc<dyn Node>,
        b: Arc<dyn Node>,
    }
    impl Node for Composite {
        fn new_instance(&self) -> Arc<dyn Node> {
            compose(self.a.new_instance(), self.b.new_instance())
        }
        fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream {
            let a_out = self.a.stream(upstream, ctx);
            self.b.stream(a_out, ctx)
        }
    }
    Arc::new(Composite { a, b })
}

pub fn with_observer(node: Arc<dyn Node>) -> Arc<dyn Node> {
    struct Observed {
        node: Arc<dyn Node>,
    }
    impl Node for Observed {
        fn new_instance(&self) -> Arc<dyn Node> {
            with_observer(self.node.new_instance())
        }
        fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream {
            let before = with_broadcast(upstream, TapPoint::Before, ctx);
            let observed = with_broadcast(self.node.stream(before, ctx), TapPoint::After, ctx);
            // `Immediate` 节点：进程完成（本叶输出流消费到 None）时释放，恰好一次。
            // 因 `compose_chain` 对每叶恰好包一次 with_observer，嵌套 compose 不会重复触发。
            if self.node.release_mode() == ReleaseMode::Immediate {
                let node = self.node.clone();
                Box::pin(futures::stream::unfold(
                    (observed, true),
                    move |(mut s, active)| {
                        let node = node.clone();
                        async move {
                            if !active {
                                return None;
                            }
                            match s.next().await {
                                Some(item) => Some((item, (s, true))),
                                None => {
                                    node.on_release();
                                    None
                                }
                            }
                        }
                    },
                ))
            } else {
                observed
            }
        }
    }
    Arc::new(Observed { node })
}

pub fn compose_chain(nodes: Vec<Arc<dyn Node>>) -> Option<Arc<dyn Node>> {
    nodes.into_iter().map(with_observer).reduce(compose)
}

/// 把 `feed_rx` 转成链首上游（投喂式 push→pull）。
fn receiver_stream(rx: tokio::sync::mpsc::UnboundedReceiver<PipelineEvent>) -> EventStream {
    Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|ev| (Ok(ev), rx))
    }))
}

/// 统一链：持有单条链（`Arc<dyn Node>`）及其头部事件投喂器（`feed` 推、`stream` 拉，二者唯一）。
/// 内部状态置于 `Arc` 中，故可 `Clone`：Session 用一份调 `begin`/`feed`，Round task 用 clone 调 `finish`。
#[derive(Clone)]
pub struct NodeChain {
    inner: Arc<NodeChainInner>,
}

struct NodeChainInner {
    head: Arc<dyn Node>,
    /// 构链时的叶子实例（有序）。`begin` 全量 `on_acquire`；`finish` 仅对 `Deferred` 叶子 `on_release`。
    leaves: Vec<Arc<dyn Node>>,
    feed_tx: tokio::sync::mpsc::UnboundedSender<PipelineEvent>,
    feed_rx: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<PipelineEvent>>>,
}

impl NodeChain {
    pub fn new(head: Arc<dyn Node>, leaves: Vec<Arc<dyn Node>>) -> Self {
        let (feed_tx, feed_rx) = tokio::sync::mpsc::unbounded_channel::<PipelineEvent>();
        Self {
            inner: Arc::new(NodeChainInner {
                head,
                leaves,
                feed_tx,
                feed_rx: std::sync::Mutex::new(Some(feed_rx)),
            }),
        }
    }

    /// 取链尾事件流；只可调用一次（receiver 唯一），重复调用返回空流。
    pub fn stream(&self, ctx: &NodeContext) -> EventStream {
        let rx = self.inner.feed_rx.lock().expect("chain lock").take();
        match rx {
            Some(rx) => self.inner.head.stream(receiver_stream(rx), ctx),
            None => Box::pin(futures::stream::empty()),
        }
    }

    pub fn feed(&self, event: PipelineEvent) {
        let _ = self.inner.feed_tx.send(event);
    }

    /// 整轮起始：对全部叶子 `on_acquire`（`VadNode` 取用池实例等）。
    pub fn begin(&self) {
        for leaf in &self.inner.leaves {
            leaf.on_acquire();
        }
    }

    /// 整轮收尾：仅对 `Deferred` 叶子 `on_release`（`Immediate` 已在 `with_observer` 流末释放，分流不重复）。
    pub fn finish(&self) {
        for leaf in &self.inner.leaves {
            if leaf.release_mode() == ReleaseMode::Deferred {
                leaf.on_release();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn with_observer_broadcasts_before_and_after_once_per_leaf() {
        struct A; // AudioFrame -> PcmFrame
        impl Node for A {
            fn new_instance(&self) -> Arc<dyn Node> {
                Arc::new(A)
            }
            fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
                Box::pin(upstream.map(|r| match r {
                    Ok(PipelineEvent::AudioFrame(_)) => {
                        Ok(PipelineEvent::PcmFrame(vec![1.0], 16000))
                    }
                    Ok(other) => Ok(other),
                    Err(e) => Err(e),
                }))
            }
        }
        struct B; // PcmFrame -> PcmFrame(翻倍)
        impl Node for B {
            fn new_instance(&self) -> Arc<dyn Node> {
                Arc::new(B)
            }
            fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
                Box::pin(upstream.map(|r| match r {
                    Ok(PipelineEvent::PcmFrame(s, rate)) => Ok(PipelineEvent::PcmFrame(
                        s.iter().map(|x| x * 2.0).collect(),
                        rate,
                    )),
                    Ok(other) => Ok(other),
                    Err(e) => Err(e),
                }))
            }
        }
        let mut ctx = NodeContext::new(CancellationToken::new());
        let (emit, mut rx) = EventSink::channel();
        ctx.emit = emit;
        // 每个叶子用 with_observer 包一次，compose 纯 pipe 相接
        let chain = NodeChain::new(
            compose_chain(vec![
                Arc::new(A) as Arc<dyn Node>,
                Arc::new(B) as Arc<dyn Node>,
            ])
            .expect("chain"),
            vec![],
        );
        let mut out = chain.stream(&ctx);
        chain.feed(PipelineEvent::AudioFrame(vec![1]));
        let first = out.next().await.expect("item").unwrap();
        match first {
            PipelineEvent::PcmFrame(s, 16000) => assert_eq!(s, vec![2.0]),
            other => panic!("expected PcmFrame, got {other:?}"),
        }
        // A 输入 Before(AudioFrame)、A 产出 After(PcmFrame)、B 输入 Before(PcmFrame)、B 产出 After(PcmFrame)
        let tagged = rx.recv().await.expect("tagged");
        assert_eq!(tagged.point, TapPoint::Before);
        assert!(matches!(tagged.event, PipelineEvent::AudioFrame(_)));
        let tagged = rx.recv().await.expect("tagged");
        assert_eq!(tagged.point, TapPoint::After);
        assert!(matches!(tagged.event, PipelineEvent::PcmFrame(_, 16000)));
        let tagged = rx.recv().await.expect("tagged");
        assert_eq!(tagged.point, TapPoint::Before);
        assert!(matches!(tagged.event, PipelineEvent::PcmFrame(_, 16000)));
        let tagged = rx.recv().await.expect("tagged");
        assert_eq!(tagged.point, TapPoint::After);
        assert!(matches!(tagged.event, PipelineEvent::PcmFrame(_, 16000)));
    }

    #[tokio::test]
    async fn nested_compose_does_not_duplicate_broadcast() {
        // Producer 在 A 处把 AudioFrame 换成 TurnComplete（模拟 turn 产出回合边界事件）。
        // 下游 Consumer 模拟 ling：消费 TurnComplete，产出 TextChunk（不透传 TurnComplete）。
        // 因此 TurnComplete 应只在 Producer 的 After 广播一次。
        struct Producer;
        impl Node for Producer {
            fn new_instance(&self) -> Arc<dyn Node> {
                Arc::new(Producer)
            }
            fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
                Box::pin(upstream.map(|r| match r {
                    Ok(PipelineEvent::AudioFrame(_)) => Ok(PipelineEvent::TurnComplete {
                        text: String::new(),
                        prob: 1.0,
                    }),
                    other => other,
                }))
            }
        }
        struct Consumer;
        impl Node for Consumer {
            fn new_instance(&self) -> Arc<dyn Node> {
                Arc::new(Consumer)
            }
            fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
                Box::pin(upstream.map(|r| match r {
                    Ok(PipelineEvent::TurnComplete { text, .. }) => Ok(PipelineEvent::TextChunk {
                        text,
                        emotion: None,
                    }),
                    other => other,
                }))
            }
        }
        let mut ctx = NodeContext::new(CancellationToken::new());
        let (emit, mut rx) = EventSink::channel();
        ctx.emit = emit;
        // compose( compose( observer(Producer), observer(Consumer) ), observer(Consumer) )
        let chain = NodeChain::new(
            compose(
                compose(
                    with_observer(Arc::new(Producer)),
                    with_observer(Arc::new(Consumer)),
                ),
                with_observer(Arc::new(Consumer)),
            ),
            vec![],
        );
        let mut out = chain.stream(&ctx);
        chain.feed(PipelineEvent::AudioFrame(vec![1]));
        out.next().await;
        let mut after_tc = 0;
        while let Ok(tagged) = rx.try_recv() {
            if tagged.point == TapPoint::After
                && matches!(tagged.event, PipelineEvent::TurnComplete { .. })
            {
                after_tc += 1;
            }
        }
        assert_eq!(
            after_tc, 1,
            "TurnComplete should broadcast After exactly once, got {after_tc}"
        );
    }

    #[tokio::test]
    async fn new_instance_reproduces_fresh_instance_and_compose_chain_broadcasts_once() {
        struct Template {
            shared: Arc<std::sync::atomic::AtomicUsize>,
        }
        impl Node for Template {
            fn new_instance(&self) -> Arc<dyn Node> {
                Arc::new(Instance {
                    counter: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    shared: self.shared.clone(),
                })
            }
            fn stream(&self, _upstream: EventStream, _ctx: &NodeContext) -> EventStream {
                panic!("template must not run stream")
            }
        }
        struct Instance {
            counter: Arc<std::sync::atomic::AtomicUsize>,
            shared: Arc<std::sync::atomic::AtomicUsize>,
        }
        impl Node for Instance {
            fn new_instance(&self) -> Arc<dyn Node> {
                unreachable!("instances are not cloned again in this test")
            }
            fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
                use futures::stream::StreamExt as _;
                let c = self.counter.clone();
                let s = self.shared.clone();
                Box::pin(upstream.map(move |r| {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    s.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    r
                }))
            }
        }
        let shared = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let template: Arc<dyn Node> = Arc::new(Template {
            shared: shared.clone(),
        });

        // 模板产出一个独立实例（实例计数器各自独立，共享计数器会累加两次）。
        let a = template.new_instance();
        let b = template.new_instance();
        let mut ctx = NodeContext::new(CancellationToken::new());
        let (emit, mut rx) = EventSink::channel();
        ctx.emit = emit;

        // 每个实例分别跑一条链；每条链经 compose_chain 包一次 with_observer → 每事件每叶子广播 2 次。
        let chain_a = NodeChain::new(compose_chain(vec![a.clone()]).expect("chain"), vec![]);
        let mut out_a = chain_a.stream(&ctx);
        chain_a.feed(PipelineEvent::PcmFrame(vec![1.0], 16000));
        out_a.next().await;
        let mut before_count_a = 0;
        while let Ok(tagged) = rx.try_recv() {
            if tagged.point == TapPoint::Before {
                before_count_a += 1;
            }
        }

        let chain_b = NodeChain::new(compose_chain(vec![b.clone()]).expect("chain"), vec![]);
        let mut out_b = chain_b.stream(&ctx);
        chain_b.feed(PipelineEvent::PcmFrame(vec![1.0], 16000));
        out_b.next().await;
        let mut before_count_b = 0;
        while let Ok(tagged) = rx.try_recv() {
            if tagged.point == TapPoint::Before {
                before_count_b += 1;
            }
        }

        // 每链各 1 个叶子，故各广播 1 次 Before；两个实例共享计数器递增 2。
        assert_eq!(before_count_a, 1);
        assert_eq!(before_count_b, 1);
        assert_eq!(shared.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// `Immediate` 叶在流消费到 `None` 时 `on_release` 恰好一次；`Deferred` 叶则需 `NodeChain::finish`，
    /// 且 `finish` 不重复释放 `Immediate` 叶。
    #[tokio::test]
    async fn lifecycle_immediate_releases_at_stream_end_deferred_at_finish() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Released {
            acquire: AtomicUsize,
            release: AtomicUsize,
            mode: ReleaseMode,
        }
        impl Node for Released {
            fn new_instance(&self) -> Arc<dyn Node> {
                unreachable!("templates not cloned in this test")
            }
            fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
                upstream
            }
            fn release_mode(&self) -> ReleaseMode {
                self.mode
            }
            fn on_acquire(&self) {
                self.acquire.fetch_add(1, Ordering::SeqCst);
            }
            fn on_release(&self) {
                self.release.fetch_add(1, Ordering::SeqCst);
            }
        }

        let mut ctx = NodeContext::new(CancellationToken::new());
        let (emit, _rx) = EventSink::channel();
        ctx.emit = emit;

        // Immediate 叶：直接以有界流（喂 1 项后结束）驱动 with_observer，验证流末释放恰好一次。
        let immediate = Arc::new(Released {
            acquire: AtomicUsize::new(0),
            release: AtomicUsize::new(0),
            mode: ReleaseMode::Immediate,
        });
        let leaf = with_observer(immediate.clone());
        let mut out = leaf.stream(
            Box::pin(futures::stream::iter([
                Ok::<_, framework::error::AppError>(PipelineEvent::PcmFrame(vec![1.0], 16000)),
            ])),
            &ctx,
        );
        assert!(out.next().await.is_some());
        while out.next().await.is_some() {}
        assert_eq!(
            immediate.release.load(Ordering::SeqCst),
            1,
            "Immediate leaf released exactly once at stream end"
        );

        // Mixed chain：Deferred 叶由 finish 统一释放，Immediate 叶不重复释放。
        let immediate2 = Arc::new(Released {
            acquire: AtomicUsize::new(0),
            release: AtomicUsize::new(0),
            mode: ReleaseMode::Immediate,
        });
        let deferred = Arc::new(Released {
            acquire: AtomicUsize::new(0),
            release: AtomicUsize::new(0),
            mode: ReleaseMode::Deferred,
        });
        let leaves = vec![
            immediate2.clone() as Arc<dyn Node>,
            deferred.clone() as Arc<dyn Node>,
        ];
        let chain = NodeChain::new(
            compose_chain(leaves.clone()).expect("chain"),
            leaves.clone(),
        );
        chain.begin();
        assert_eq!(immediate2.acquire.load(Ordering::SeqCst), 1);
        assert_eq!(deferred.acquire.load(Ordering::SeqCst), 1);
        chain.finish();
        assert_eq!(
            deferred.release.load(Ordering::SeqCst),
            1,
            "Deferred leaf released by NodeChain::finish"
        );
        assert_eq!(
            immediate2.release.load(Ordering::SeqCst),
            0,
            "Immediate leaf not released by finish (only at stream end)"
        );
    }
}
