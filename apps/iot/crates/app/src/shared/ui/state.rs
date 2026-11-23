//! The interaction states a widget is drawn in.

/// Bits a page derives from its snapshot and hands to a widget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State(u8);

impl State {
    pub const NONE: Self = Self(0);
    pub const PRESSED: Self = Self(1);
    pub const SELECTED: Self = Self(2);
    pub const DISABLED: Self = Self(4);
    pub const ERROR: Self = Self(8);
    pub const DRAGGED: Self = Self(16);
    pub const FOCUSED: Self = Self(32);
    pub const SCROLLED_UNDER: Self = Self(64);

    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Adds `other` when `on`; keeps the value otherwise.
    pub const fn then(self, other: Self, on: bool) -> Self {
        if on { self.union(other) } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::State;

    #[test]
    fn a_state_is_a_bitset() {
        let state = State::NONE
            .then(State::PRESSED, true)
            .then(State::DISABLED, false);
        assert!(state.has(State::PRESSED));
        assert!(!state.has(State::DISABLED));
        assert_eq!(state.union(State::SELECTED), State(State::PRESSED.0 | 2));
    }
}
