use std::sync::{Arc, Mutex};

use crate::component::vad::VadManager;
use crate::config::vad::VadConfig;
use service::component::vad::{Vad, VadPool as VadPoolTrait};

/// VAD 对象池：串行状态机对象按需复用（取用前 `clear()`，用后归还）。
/// 保证同一时刻仅被一个 Round 持有（acquire/release 严格配对），避免并发破坏 `is_speech`。
/// 以 `Arc<dyn service::component::vad::VadPool>` 共享给多个 `VadNode`，由节点 RAII 归还。
#[derive(Clone)]
pub struct VadPool {
    free: Arc<Mutex<Vec<Box<dyn Vad>>>>,
    config: Arc<VadConfig>,
}

impl VadPool {
    pub fn new(config: Arc<VadConfig>) -> Self {
        Self {
            free: Arc::new(Mutex::new(Vec::new())),
            config,
        }
    }
}

impl VadPoolTrait for VadPool {
    /// 取一个 VAD 对象；池空则新建，取用前一律 `clear()` 以消除跨 round 残留状态。
    fn acquire(&self) -> Box<dyn Vad> {
        let mut free = self.free.lock().expect("vad pool lock");
        let mut vad = free
            .pop()
            .unwrap_or_else(|| VadManager::create_model(&self.config));
        vad.clear();
        vad
    }

    /// 归还 VAD 对象供复用。
    fn release(&self, vad: Box<dyn Vad>) {
        self.free.lock().expect("vad pool lock").push(vad);
    }
}
