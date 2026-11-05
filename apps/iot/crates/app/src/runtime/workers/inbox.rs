//! The two shapes every handle is made of: an arrival queue and a retained value. Neither
//! knows which worker it belongs to.

use core::cell::Cell;
use core::future::Future;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, TrySendError};
use embassy_sync::signal::Signal;

/// One worker's arrival queue: entries keep their order, and the verb says what a full queue
/// does — [`Inbox::publish`] gives up the oldest entry, [`Inbox::send`] waits for room. One
/// queue per consumer, so the overflow policy belongs to the queue rather than to each call
/// site.
pub struct Inbox<T, const DEPTH: usize> {
    ring: Channel<CriticalSectionRawMutex, T, DEPTH>,
}

impl<T: Send, const DEPTH: usize> Inbox<T, DEPTH> {
    pub(crate) const fn new() -> Self {
        Self {
            ring: Channel::new(),
        }
    }

    /// Never blocks and never fails: a full ring gives up its oldest entry to make room, which
    /// is the `keep last` history of DDS. The eviction and the retry are one synchronous step,
    /// so one publisher per inbox keeps the second `try_send` unreachable; a producer that
    /// cannot lose its message waits with [`Self::send`] instead.
    pub fn publish(&self, message: T) {
        if let Err(TrySendError::Full(message)) = self.ring.try_send(message) {
            let _ = self.ring.try_receive();
            let _ = self.ring.try_send(message);
        }
    }

    /// Waits for room: what arrives is not given up for a later message. The phone's join goes
    /// this way, because its page parks as soon as it has asked.
    pub async fn send(&self, message: T) {
        self.ring.send(message).await;
    }

    pub async fn receive(&self) -> T {
        self.ring.receive().await
    }

    pub fn try_receive(&self) -> Option<T> {
        self.ring.try_receive().ok()
    }

    /// Drops what `keep` refuses and puts the rest back in order. A publish that lands mid-way
    /// is newer than anything here, so it may take the room a returning entry was going into;
    /// that is this queue's policy, not a hole.
    pub fn retain(&self, keep: impl Fn(&T) -> bool) {
        let mut kept: [Option<T>; DEPTH] = core::array::from_fn(|_| None);
        let mut held = 0;
        while let Some(message) = self.try_receive() {
            if !keep(&message) {
                continue;
            }
            if let Some(slot) = kept.get_mut(held) {
                *slot = Some(message);
                held += 1;
            }
        }
        for message in kept.into_iter().flatten() {
            let _ = self.ring.try_send(message);
        }
    }
}

/// The newest value only: the panel draws one list, so a burst of scans is one list plus the
/// news that it changed. Reading does not consume it, which is what makes it a retained value
/// rather than a message: the panel and the phone's page both read the same list. Exactly one
/// reader takes; any number may peek.
pub struct Retained<T> {
    value: Mutex<CriticalSectionRawMutex, Cell<Option<T>>>,
    fresh: Mutex<CriticalSectionRawMutex, Cell<bool>>,
    arrived: Signal<CriticalSectionRawMutex, ()>,
}

impl<T: Copy + Send> Retained<T> {
    pub(crate) const fn new() -> Self {
        Self {
            value: Mutex::new(Cell::new(None)),
            fresh: Mutex::new(Cell::new(false)),
            arrived: Signal::new(),
        }
    }

    /// Takes the newest value once per publish, leaving it in place: the panel draws it and the
    /// phone's page reads the same list with [`Self::peek`], so a taken list is not a gone list.
    pub fn take(&self) -> Option<T> {
        if !self.fresh.lock(|slot| slot.replace(false)) {
            return None;
        }
        self.value.lock(|slot| slot.get())
    }

    /// Reads the newest value without taking it: setup mode's page and the panel draw the same
    /// list, and the phone's request is not a consumer.
    pub fn peek(&self) -> Option<T> {
        self.value.lock(|slot| slot.get())
    }

    pub fn ready(&self) -> impl Future<Output = ()> {
        self.arrived.wait()
    }

    /// By reference: the whole list would otherwise be copied into the scan future's frame.
    pub fn publish(&self, value: &T) {
        self.value.lock(|slot| slot.set(Some(*value)));
        self.fresh.lock(|slot| slot.set(true));
        self.arrived.signal(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use core::task::{Context, Waker};

    fn cx() -> Context<'static> {
        Context::from_waker(Waker::noop())
    }

    #[test]
    fn a_value_published_before_the_last_is_taken_replaces_it_rather_than_being_refused() {
        let retained = Retained::new();
        retained.publish(&1);
        retained.publish(&2);

        assert_eq!(
            retained.take(),
            Some(2),
            "the newest value is the one handed over"
        );
        assert_eq!(
            retained.take(),
            None,
            "and it is handed over once per publish"
        );
        assert_eq!(
            retained.peek(),
            Some(2),
            "the copy stays for the readers that only peek"
        );
    }

    #[test]
    fn the_wake_a_park_waits_on_does_not_take_the_value_it_woke_for() {
        let retained = Retained::new();
        retained.publish(&7);
        let mut wake = core::pin::pin!(retained.ready());
        assert!(
            wake.as_mut().poll(&mut cx()).is_ready(),
            "the wake is up as soon as a value is waiting"
        );
        assert_eq!(
            retained.take(),
            Some(7),
            "the take at the top of the pass is the one that takes the value"
        );
    }

    #[test]
    fn a_full_inbox_gives_up_the_oldest_entry_and_keeps_the_order_of_the_rest() {
        let inbox = Inbox::<u8, 2>::new();
        inbox.publish(1);
        inbox.publish(2);
        inbox.publish(3);

        assert_eq!(
            inbox.try_receive(),
            Some(2),
            "the oldest entry made room for the newest, so 1 is gone"
        );
        assert_eq!(
            inbox.try_receive(),
            Some(3),
            "and the rest kept their order"
        );
        assert_eq!(inbox.try_receive(), None, "the ring is empty");
    }

    #[test]
    fn publishing_never_fails_or_waits_even_when_the_ring_is_full() {
        let inbox = Inbox::<u8, 4>::new();
        for value in 0..8 {
            inbox.publish(value);
        }

        let kept: Vec<u8> = core::iter::from_fn(|| inbox.try_receive()).collect();
        assert_eq!(
            kept,
            [4, 5, 6, 7],
            "a burst deeper than the ring keeps the newest entries, in order"
        );
    }

    #[test]
    fn a_sending_publisher_waits_for_room_instead_of_giving_up_an_entry() {
        let inbox = Inbox::<u8, 2>::new();
        inbox.publish(1);
        inbox.publish(2);

        let mut sent = core::pin::pin!(inbox.send(3));
        assert!(
            sent.as_mut().poll(&mut cx()).is_pending(),
            "the ring is full, so the sender waits"
        );
        assert_eq!(
            inbox.try_receive(),
            Some(1),
            "and nothing was given up for it"
        );

        assert!(
            sent.as_mut().poll(&mut cx()).is_ready(),
            "the room the receive freed is the room the sender takes"
        );
        assert_eq!(inbox.try_receive(), Some(2));
        assert_eq!(inbox.try_receive(), Some(3));
    }

    #[test]
    fn retaining_a_message_drops_the_others_and_keeps_the_order() {
        let inbox = Inbox::<u8, 4>::new();
        for value in 1..=4 {
            inbox.publish(value);
        }

        inbox.retain(|value| *value % 2 == 0);

        assert_eq!(inbox.try_receive(), Some(2));
        assert_eq!(inbox.try_receive(), Some(4));
        assert_eq!(inbox.try_receive(), None);
    }
}
