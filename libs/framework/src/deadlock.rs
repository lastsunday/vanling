use std::{thread, time::Duration};

pub fn deadlock_detection_thread() {
    loop {
        thread::sleep(Duration::from_secs(10));
        let deadlocks = parking_lot::deadlock::check_deadlock();
        if deadlocks.is_empty() {
            continue;
        }

        eprintln!("{} deadlocks detected", deadlocks.len());
        for (i, threads) in deadlocks.iter().enumerate() {
            eprintln!("Deadlock #{i}");
            for t in threads {
                eprintln!("Thread Id {:#?}", t.thread_id());
                eprintln!("{:#?}", t.backtrace());
            }
        }
    }
}

pub fn spawn() {
    thread::Builder::new()
        .name("deadlock_detector".to_owned())
        .spawn(deadlock_detection_thread)
        .expect("failed to spawn deadlock detection thread");
}
