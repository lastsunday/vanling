use service::ws::frame::Frame;
use tokio::sync::mpsc::UnboundedSender;

pub trait InputSender: Send + Sync {
    fn send(&self, msg: Frame);
}

impl InputSender for UnboundedSender<Frame> {
    fn send(&self, msg: Frame) {
        let _ = self.send(msg);
    }
}
