//! Whether a wired part is answering right now.

/// What a worker last found when it touched a part. Presence is the board's wiring and never
/// changes; this is the runtime half: a part can go down and come back without its page
/// leaving the cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Health {
    /// The part answered and can carry its function.
    #[default]
    Ready,
    /// The part did not answer; the board's own words say why, and the page shows them.
    Down(&'static str),
}

impl Health {
    pub const fn fault(self) -> Option<&'static str> {
        match self {
            Self::Ready => None,
            Self::Down(reason) => Some(reason),
        }
    }

    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}
