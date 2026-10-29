//! Who this board is, answered by the parts themselves: each driver that can answer a
//! question reports its own name, address and evidence, so the board cannot write a line no
//! hardware backed.

#[cfg(target_os = "none")]
use esp_hal::efuse::{base_mac_address, chip_revision};

use core::fmt;

use alloc::boxed::Box;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    Read,
    Declared,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Present,
    Degraded(&'static str),
    Absent,
}

/// Fields are private so a status can only come from one of the four constructors: a caller
/// that could build `Present`/`Declared` by hand could report a part it never asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Status {
    state: State,
    evidence: Evidence,
}

impl Status {
    pub const fn answered() -> Self {
        Self {
            state: State::Present,
            evidence: Evidence::Read,
        }
    }

    pub const fn silent() -> Self {
        Self {
            state: State::Absent,
            evidence: Evidence::Read,
        }
    }

    pub const fn refused(reason: &'static str) -> Self {
        Self {
            state: State::Degraded(reason),
            evidence: Evidence::Read,
        }
    }

    pub const fn declared() -> Self {
        Self {
            state: State::Present,
            evidence: Evidence::Declared,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Address(u8);

impl Address {
    pub const fn new(byte: u8) -> Self {
        Self(byte)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        let text = [
            b'0',
            b'x',
            DIGITS[(self.0 >> 4) as usize],
            DIGITS[(self.0 & 0xF) as usize],
        ];
        f.pad(core::str::from_utf8(&text).unwrap_or_default())
    }
}

pub trait Named {
    const NAME: &'static str;
}

pub trait SelfCheck {
    fn name(&self) -> &'static str;

    fn id(&self) -> Option<Address> {
        None
    }

    fn check(&mut self) -> Status;
}

impl<T: SelfCheck> SelfCheck for Box<T> {
    fn name(&self) -> &'static str {
        (**self).name()
    }

    fn id(&self) -> Option<Address> {
        (**self).id()
    }

    fn check(&mut self) -> Status {
        (**self).check()
    }
}

impl<T: Named> Named for Box<T> {
    const NAME: &'static str = T::NAME;
}

#[cfg(target_os = "none")]
pub fn identity() {
    let revision = chip_revision();
    let mac = base_mac_address();
    let mac = mac.as_bytes();
    log::info!(
        "[HW] identity chip revision v{}.{}, mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        revision.major,
        revision.minor,
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5],
    );
}

pub struct Report {
    present: usize,
    degraded: usize,
    absent: usize,
    read: usize,
    declared: usize,
}

impl Report {
    pub fn of<const N: usize>(parts: [&mut dyn SelfCheck; N]) -> Self {
        let mut report = Self::blank();
        for part in parts {
            report.ask(part);
        }
        report
    }

    pub fn include<T: SelfCheck + Named>(mut self, part: Option<&mut T>) -> Self {
        match part {
            Some(part) => self.ask(part),
            None => self.record(T::NAME, None, Status::silent()),
        }
        self
    }

    pub fn finish(self) {
        log::info!("{}", self.summary());
    }

    const fn blank() -> Self {
        Self {
            present: 0,
            degraded: 0,
            absent: 0,
            read: 0,
            declared: 0,
        }
    }

    fn ask(&mut self, part: &mut dyn SelfCheck) {
        let name = part.name();
        let id = part.id();
        let status = part.check();
        self.record(name, id, status);
    }

    fn record(&mut self, name: &str, id: Option<Address>, status: Status) {
        match status.state {
            State::Present => self.present += 1,
            State::Degraded(_) => self.degraded += 1,
            State::Absent => self.absent += 1,
        }
        match status.evidence {
            Evidence::Read => self.read += 1,
            Evidence::Declared => self.declared += 1,
        }
        let line = Line { name, id, status };
        match status.state {
            State::Degraded(_) => log::warn!("{line}"),
            State::Present | State::Absent => log::info!("{line}"),
        }
    }

    fn summary(&self) -> Summary {
        Summary {
            present: self.present,
            degraded: self.degraded,
            absent: self.absent,
            read: self.read,
            declared: self.declared,
        }
    }
}

struct Line<'a> {
    name: &'a str,
    id: Option<Address>,
    status: Status,
}

impl fmt::Display for Line<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[HW] {:<10}", self.name)?;
        match self.id {
            Some(id) => write!(f, "{id:<8}")?,
            None => write!(f, "{:<8}", "")?,
        }
        match self.status.state {
            State::Present => write!(f, "{:<13}", "present")?,
            State::Absent => write!(f, "{:<13}", "absent")?,
            State::Degraded(reason) => write!(f, "degraded: {reason} ")?,
        }
        match self.status.evidence {
            Evidence::Read => write!(f, "(read)"),
            Evidence::Declared => write!(f, "(declared)"),
        }
    }
}

struct Summary {
    present: usize,
    degraded: usize,
    absent: usize,
    read: usize,
    declared: usize,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = self.present + self.degraded + self.absent;
        write!(
            f,
            "[HW] {total} devices: {} present, {} degraded, {} absent; {} read, {} declared",
            self.present, self.degraded, self.absent, self.read, self.declared
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use core::cell::Cell;

    struct Probe {
        name: &'static str,
        id: Option<Address>,
        status: Status,
        asked: Cell<usize>,
    }

    impl Probe {
        fn new(name: &'static str, status: Status) -> Self {
            Self {
                name,
                id: None,
                status,
                asked: Cell::new(0),
            }
        }
    }

    impl Named for Probe {
        const NAME: &'static str = "probe";
    }

    impl SelfCheck for Probe {
        fn name(&self) -> &'static str {
            self.name
        }

        fn id(&self) -> Option<Address> {
            self.id
        }

        fn check(&mut self) -> Status {
            self.asked.set(self.asked.get() + 1);
            self.status
        }
    }

    #[test]
    fn a_status_carries_the_state_and_the_evidence_it_came_from() {
        for (status, state, evidence) in [
            (Status::answered(), State::Present, Evidence::Read),
            (Status::silent(), State::Absent, Evidence::Read),
            (
                Status::refused("no chip id"),
                State::Degraded("no chip id"),
                Evidence::Read,
            ),
            (Status::declared(), State::Present, Evidence::Declared),
        ] {
            assert_eq!((status.state, status.evidence), (state, evidence));
        }
    }

    #[test]
    fn a_report_asks_each_part_exactly_once() {
        let mut first = Probe::new("first", Status::answered());
        let mut second = Probe::new("second", Status::silent());
        let _ = Report::of([&mut first, &mut second]).summary();
        assert_eq!(first.asked.get(), 1);
        assert_eq!(second.asked.get(), 1);
    }

    #[test]
    fn an_empty_wired_slot_is_absent_without_asking_anything() {
        let mut fitted = Probe::new("mic", Status::answered());
        let mut empty: Option<Probe> = None;
        let report = Report::of([&mut fitted]).include(empty.as_mut());
        assert_eq!(fitted.asked.get(), 1);
        assert_eq!(
            report.summary().to_string(),
            "[HW] 2 devices: 1 present, 0 degraded, 1 absent; 2 read, 0 declared"
        );
    }

    #[test]
    fn a_line_is_name_id_state_evidence() {
        let line = Line {
            name: "touch",
            id: Some(Address::new(0x38)),
            status: Status::answered(),
        };
        assert_eq!(
            line.to_string(),
            "[HW] touch     0x38    present      (read)"
        );

        let line = Line {
            name: "panel",
            id: None,
            status: Status::declared(),
        };
        assert_eq!(
            line.to_string(),
            "[HW] panel             present      (declared)"
        );
    }

    #[test]
    fn a_reason_is_printed_with_the_state_that_carries_it() {
        let line = Line {
            name: "speaker",
            id: Some(Address::new(0x18)),
            status: Status::refused("not an ES8311"),
        };
        assert_eq!(
            line.to_string(),
            "[HW] speaker   0x18    degraded: not an ES8311 (read)"
        );
    }

    #[test]
    fn an_address_is_written_the_way_a_datasheet_writes_it() {
        assert_eq!(Address::new(0x38).to_string(), "0x38");
        assert_eq!(Address::new(0x6B).to_string(), "0x6B");
        assert_eq!(Address::new(0x00).to_string(), "0x00");
    }

    #[test]
    fn the_summary_is_the_lines_it_printed() {
        let mut panel = Probe::new("panel", Status::declared());
        let mut mic = Probe::new("mic", Status::silent());
        let mut speaker = Probe::new("speaker", Status::refused("no codec"));
        let report = Report::of([&mut panel, &mut mic, &mut speaker]);
        assert_eq!(
            report.summary().to_string(),
            "[HW] 3 devices: 1 present, 1 degraded, 1 absent; 2 read, 1 declared"
        );
    }
}
