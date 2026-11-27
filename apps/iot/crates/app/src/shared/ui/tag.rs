//! Tags: the numbers a page draws on a control and reads back from a finger.

/// A control set whose values are its wire tags. `ALL` lists every variant, so a tag drawn on
/// the panel can always be decoded; a variant that is never declared (a dialog's inner targets)
/// is still legal.
pub trait Tag: Copy + PartialEq + 'static {
    const ALL: &'static [Self];

    fn tag(self) -> u16;

    fn from_tag(tag: u16) -> Option<Self>
    where
        Self: Sized,
    {
        Self::ALL.iter().copied().find(|value| value.tag() == tag)
    }
}
