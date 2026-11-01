//! The glass: one controller, one backlight, and the write that reaches them.
//!
//! The controller does not run on the coordinator's core. Its transfer is a blocking DMA
//! call, so it holds whatever executor it runs on for the frame's ~16 ms; the coordinator
//! posts a frame and awaits the answer instead, and the second core spends the 16 ms.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use esp_hal::spi::Error;

use crate::components::backlight::Backlight;
use crate::components::st7789::{St7789, Wire};

/// The frame the coordinator has just finished painting, handed over without a borrow.
struct Ticket {
    ptr: *const u8,
    len: usize,
}

// SAFETY: the bytes are the `'static` frame arena the screen draws into, and the handshake
// is strict: [`Panel::write`] posts one ticket and then parks in `SHIPPED.wait()` until it
// answers, so the coordinator is not touching them for the ticket's whole life and the bus
// only reads through the pointer. One screen, one request in flight.
unsafe impl Send for Ticket {}

static REQUESTS: Channel<CriticalSectionRawMutex, Ticket, 1> = Channel::new();

static SHIPPED: Signal<CriticalSectionRawMutex, Result<Wire, Error>> = Signal::new();

/// The controller's half: it owns the bus and answers one frame at a time.
pub struct PanelBus {
    driver: St7789,
}

impl PanelBus {
    /// Answers frames for the rest of the boot. Each request is one whole frame.
    pub async fn run(mut self) -> ! {
        loop {
            let ticket = REQUESTS.receive().await;
            // SAFETY: `Ticket`'s own — the bytes outlive the transfer, and the coordinator
            // does not write them until this answers.
            let frame = unsafe { core::slice::from_raw_parts(ticket.ptr, ticket.len) };
            SHIPPED.signal(self.driver.write_frame(frame).await);
        }
    }
}

pub(super) struct Panel {
    width: u16,
    height: u16,
    backlight: Backlight,
    ceiling: u8,
    requested: u8,
    lit: bool,
}

impl Panel {
    pub(super) fn new(driver: St7789, backlight: Backlight) -> (Self, PanelBus) {
        let (width, height) = (driver.width(), driver.height());
        (
            Self {
                width,
                height,
                backlight,
                ceiling: 100,
                requested: 0,
                lit: false,
            },
            PanelBus { driver },
        )
    }

    pub(super) fn width(&self) -> u16 {
        self.width
    }

    pub(super) fn height(&self) -> u16 {
        self.height
    }

    pub(super) fn frame_bytes(&self) -> usize {
        usize::from(self.width) * usize::from(self.height) * 2
    }

    pub(super) async fn write(&mut self, frame: &[u8]) -> Result<Wire, Error> {
        REQUESTS
            .send(Ticket {
                ptr: frame.as_ptr(),
                len: frame.len(),
            })
            .await;
        SHIPPED.wait().await
    }

    pub(super) fn set_backlight(&mut self, level_pct: u8) {
        self.requested = level_pct.min(100);
        if self.lit {
            self.backlight
                .set_level_pct(self.requested.min(self.ceiling));
        }
    }

    pub(super) fn set_ceiling(&mut self, level_pct: u8) {
        self.ceiling = level_pct.min(100);
        if self.lit {
            self.backlight
                .set_level_pct(self.requested.min(self.ceiling));
        }
    }

    pub(super) fn note_frame_shipped(&mut self) {
        if !self.lit {
            self.lit = true;
            self.backlight
                .set_level_pct(self.requested.min(self.ceiling));
        }
    }
}

impl crate::hardware::SelfCheck for Panel {
    fn name(&self) -> &'static str {
        "panel"
    }

    fn check(&mut self) -> crate::hardware::Status {
        crate::hardware::Status::declared()
    }
}

/// The bus's task, for the core whose time the transfer should cost.
#[embassy_executor::task]
pub async fn panel_service(bus: PanelBus) -> ! {
    bus.run().await
}
