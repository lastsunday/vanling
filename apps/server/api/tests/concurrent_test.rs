use std::{
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};

use tokio::time::sleep;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_atomic() {
    let count = Arc::new(AtomicI32::new(0));
    let count = count.clone();
    let count_set = count.clone();
    tokio::spawn(async move {
        loop {
            let count = count.load(Ordering::Relaxed);
            info!("load {}", count);
            sleep(Duration::from_millis(200)).await;
        }
    });
    tokio::spawn(async move {
        loop {
            let count = count_set.load(Ordering::Relaxed);
            let count = count + 1;
            count_set.store(count, Ordering::Relaxed);
            info!("set {}", count);
            sleep(Duration::from_millis(500)).await;
        }
    });
    sleep(Duration::from_millis(5000)).await;
}
