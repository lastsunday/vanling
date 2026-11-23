//! The gallery's controls, numbered once: the tag a page draws is the verb it reports, so the
//! view, the hit test and the handler cannot drift apart.

/// One control on the Widgets page. The discriminant is the wire tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum WidgetsAction {
    Filled = 1,
    Outlined,
    Bare,
    ChipA,
    ChipB,
    Switch,
    Check,
    Radio0,
    Radio1,
    Radio2,
    Slider,
    Field,
    Select,
    Icon,
    Tile,
}

impl WidgetsAction {
    pub const ALL: &'static [Self] = &[
        Self::Filled,
        Self::Outlined,
        Self::Bare,
        Self::ChipA,
        Self::ChipB,
        Self::Switch,
        Self::Check,
        Self::Radio0,
        Self::Radio1,
        Self::Radio2,
        Self::Slider,
        Self::Field,
        Self::Select,
        Self::Icon,
        Self::Tile,
    ];

    pub const fn tag(self) -> u16 {
        self as u16
    }

    pub fn from_tag(tag: u16) -> Option<Self> {
        Self::ALL.iter().copied().find(|action| action.tag() == tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tags_are_dense_and_read_back() {
        for (index, action) in WidgetsAction::ALL.iter().enumerate() {
            assert_eq!(action.tag(), index as u16 + 1);
            assert_eq!(WidgetsAction::from_tag(action.tag()), Some(*action));
        }
        assert_eq!(
            WidgetsAction::from_tag(WidgetsAction::ALL.len() as u16 + 1),
            None
        );
        assert_eq!(WidgetsAction::from_tag(0), None);
    }
}
