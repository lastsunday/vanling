//! Where the device is: the declared route table plus a fixed-depth stack of destinations.
//! A page is a destination, not a picture — a board with no panel still lands on one, and the
//! panel is only one of its readers.
//!
//! The model is the one mainstream navigators share — a destination is shown, a transient is
//! pushed over it, and back drops the top while the root stays. What differs here is only how
//! a destination is chosen: the button replaces the top (a tab, not a growing history), and
//! the boot transient is pushed over the landing page and popped when its lifetime ends.

use super::Page;
use iot_core::drivers::board::Backed;

/// How many pages can sit over the root. Two is what the device uses today (root + splash);
/// the slack leaves room for a page that opens another without unwinding the cycle.
pub const MAX_DEPTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Navigator {
    stack: [Page; MAX_DEPTH],
    depth: usize,
}

impl Default for Navigator {
    fn default() -> Self {
        Self::new(Page::default())
    }
}

impl Navigator {
    /// A stack holding only `root`.
    pub const fn new(root: Page) -> Self {
        Self {
            stack: [root; MAX_DEPTH],
            depth: 1,
        }
    }

    /// The page on screen.
    pub const fn current(&self) -> Page {
        self.stack[self.depth - 1]
    }

    /// The page the stack was opened on. It survives every pop, so this is where the device
    /// lands when the transient above it ends.
    pub const fn root(&self) -> Page {
        self.stack[0]
    }

    /// The pages on the stack, bottom first. The last one is [`Self::current`].
    pub fn entries(&self) -> &[Page] {
        &self.stack[..self.depth]
    }

    /// Show `page` in place of the current one: the top-level move, with no history behind it.
    pub fn go(&mut self, page: Page, backed: Backed) {
        self.stack[self.depth - 1] = reachable(page, backed);
    }

    /// Show `page` over the current one. At `MAX_DEPTH` the top is replaced instead, so a
    /// runaway push cannot grow the stack past its capacity.
    pub fn push(&mut self, page: Page, backed: Backed) {
        let page = reachable(page, backed);
        if self.depth == MAX_DEPTH {
            self.stack[MAX_DEPTH - 1] = page;
            return;
        }
        self.stack[self.depth] = page;
        self.depth += 1;
    }

    /// Drop the top; the root stays. Returns the page now on screen.
    pub fn pop(&mut self) -> Page {
        if self.depth > 1 {
            self.depth -= 1;
        }
        self.current()
    }

    /// The button: the cycle's next backed page, in place. A transient has nowhere to go until
    /// its lifetime ends, so this leaves it up.
    pub fn advance(&mut self, backed: Backed) -> Page {
        let next = self.current().next_page(backed);
        self.go(next, backed);
        self.current()
    }
}

/// A page the board cannot back is not somewhere the device could arrive.
fn reachable(page: Page, backed: Backed) -> Page {
    if page.is_backed(backed) {
        page
    } else {
        Page::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_BACKED: Backed = Backed {
        motion: Some(iot_core::drivers::motion::MotionCapabilities::EMPTY),
        audio: true,
        playback: true,
        camera: true,
        network: true,
    };

    #[test]
    fn a_new_stack_shows_only_its_root() {
        let nav = Navigator::new(Page::Camera);
        assert_eq!(nav.current(), Page::Camera);
        assert_eq!(nav.root(), Page::Camera);
        assert_eq!(nav.depth, 1);
    }

    #[test]
    fn the_default_stack_is_on_the_default_page() {
        assert_eq!(Navigator::default().current(), Page::default());
    }

    #[test]
    fn the_entries_are_the_stack_bottom_first() {
        let mut nav = Navigator::new(Page::Camera);
        nav.push(Page::Splash, ALL_BACKED);
        assert_eq!(
            nav.entries(),
            &[Page::Camera, Page::Splash],
            "the root is first and the page on screen last"
        );
    }

    #[test]
    fn go_replaces_the_top_without_growing_the_stack() {
        let mut nav = Navigator::new(Page::Ambient);
        nav.push(Page::Splash, ALL_BACKED);
        nav.go(Page::Camera, ALL_BACKED);
        assert_eq!(nav.current(), Page::Camera);
        assert_eq!(nav.root(), Page::Ambient);
        assert_eq!(nav.depth, 2, "the page it replaced is gone");
    }

    #[test]
    fn pop_drops_the_top_and_the_root_survives() {
        let mut nav = Navigator::new(Page::Network);
        nav.push(Page::Splash, ALL_BACKED);
        assert_eq!(nav.pop(), Page::Network);
        assert_eq!(nav.pop(), Page::Network, "the root has nowhere to go");
        assert_eq!(nav.depth, 1);
    }

    #[test]
    fn a_push_past_the_capacity_replaces_the_top() {
        let mut nav = Navigator::new(Page::Ambient);
        for page in [Page::Camera, Page::Audio, Page::Network] {
            nav.push(page, ALL_BACKED);
        }
        assert_eq!(nav.depth, MAX_DEPTH);
        nav.push(Page::Speaker, ALL_BACKED);
        assert_eq!(
            nav.depth, MAX_DEPTH,
            "the stack cannot grow past its capacity"
        );
        assert_eq!(nav.current(), Page::Speaker);
    }

    #[test]
    fn a_page_the_board_cannot_back_falls_back_to_the_root_page() {
        let bare = Backed::NONE;
        let mut nav = Navigator::new(Page::Ambient);
        nav.go(Page::Camera, bare);
        assert_eq!(nav.current(), Page::default());
        nav.push(Page::Audio, bare);
        assert_eq!(nav.current(), Page::default());
    }

    #[test]
    fn the_cycle_steps_in_place_and_skips_what_the_board_cannot_back() {
        let bare = Backed::NONE;
        let mut nav = Navigator::new(Page::Ambient);
        assert_eq!(nav.advance(bare), Page::Ambient, "nothing to turn to");
        assert_eq!(nav.depth, 1);

        let mut nav = Navigator::new(Page::Ambient);
        assert_eq!(nav.advance(ALL_BACKED), Page::Attitude);
        assert_eq!(nav.depth, 1, "the cycle is a tab, not a history");
        assert_eq!(nav.root(), Page::Attitude, "the top is the root");
    }

    #[test]
    fn a_transient_stays_up_through_a_button_press() {
        let mut nav = Navigator::new(Page::Ambient);
        nav.push(Page::Splash, ALL_BACKED);
        assert_eq!(nav.advance(ALL_BACKED), Page::Splash);
        assert_eq!(nav.depth, 2, "and does not disturb what it covers");
    }
}
