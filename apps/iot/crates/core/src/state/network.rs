//! Network setup: the join state the page walks through, and the confirmation a
//! destructive action waits on.
use super::*;

/// What the Network page is in the middle of: the row a passphrase is being typed for, the
/// draft itself, and the last refusal.
///
/// Part of the state and of the diagnostics because the page draws all of it — the list is
/// what a tap picks from, the keyboard is what picking opens, and the refusal is what the page
/// has to keep saying while the survey's own phase reports overwrite each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetworkSetup {
    /// The scanned row the user picked, whose name the draft goes with.
    pub selected: Option<AccessPoint>,
    /// The draft, and with it what a passphrase is being typed for.
    ///
    /// Kept while the keyboard is *down* as well as up: a credential the radio refused is the one
    /// a user has to edit, and a draft dropped on the way out would make them type the whole thing
    /// again to change one character.
    pub entry: Option<Entry>,
    /// Whether the keyboard is up, which is what the page's two states are.
    ///
    /// Separate from the draft because the two come apart exactly while an attempt is in flight:
    /// the credential has been handed to the radio, the page goes back to the list so the phase and
    /// the address have somewhere to be read, and what was typed is still there to come back to.
    ///
    /// Never up without a draft: the keys are drawn *from* the draft, and the page hides the strip
    /// while they are, so a keyboard with nothing under it is a page that drew nothing at all where
    /// the list and the strip should be — black, and with no control left on it. Every write of
    /// this field is therefore conditional on [`Self::entry`] being `Some`.
    pub keyboard: bool,
    /// Whether the draft is drawn as it was typed rather than masked.
    pub revealed: bool,
    /// The last association the radio refused, kept until another attempt starts.
    pub refused: Option<NetworkError>,
    /// The first scan row the list is showing. The list is [`LIST_ROWS`] rows tall and eight rows
    /// long at most, so a finger slides it a row at a time rather than seeing it all at once.
    pub list_top: u8,
    /// The confirmation the page is asking, if it is asking one.
    ///
    /// The page's third state: while it is up the list and the action strip are not drawn at all
    /// and the only thing a tap can mean is one of the two answers — see [`crate::page::Part`],
    /// which is where both halves of that rule live. Drawn, so it rides the snapshot.
    pub confirm: Option<Confirm>,
    /// Whether the action strip is waiting for a survey round it asked for.
    ///
    /// The strip's one word for "asked, not answered yet". A round is a fraction of a second of
    /// radio time and the phase it publishes is what clears this, so the strip says `WAIT` rather
    /// than inviting a second tap that would ask for the same round again.
    pub rescan_wait: bool,
}

/// The action a confirmation is standing in front of.
///
/// Two, and each is the kind of thing a page asks about before doing: a link that goes down cannot
/// be put back without the passphrase, and a forgotten network is one the next boot will not join.
/// The rest of what this page does — picking a row, typing, looking around again — is either
/// reversible or asks for nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    /// Leave the network the link is on, which is how setup mode is entered from a link.
    ///
    /// The network is the link's own, read when the question is drawn and again when it is
    /// answered, so a link that dropped under the box takes the question with it rather than
    /// taking down a link that is no longer there.
    Leave,
    /// Forget a saved network, named by the row the finger held.
    Forget(AccessPoint),
}

impl Confirm {
    /// The network this question is about, which is the row the page highlights behind it.
    ///
    /// A leave is about the link's own network, so it is read off the link rather than carried: the
    /// row the box is over is the one the link is on, and it stays that row until the question is
    /// answered. A link the radio has not accepted yet has no access point, and the question then
    /// names nothing rather than naming the wrong one.
    pub fn network(&self, link: LinkStatus) -> Option<AccessPoint> {
        match self {
            Confirm::Leave => link.ap,
            Confirm::Forget(ap) => Some(*ap),
        }
    }
}

impl NetworkSetup {
    /// The phase the page should show.
    ///
    /// The link's own phase whenever it says anything about an attempt, and the last refusal
    /// while it does not: a survey round lands within a second of a refused association and
    /// would otherwise print `Off` over the one moment the user has to be told.
    pub fn phase(&self, link: LinkPhase) -> LinkPhase {
        match (self.refused, link) {
            (Some(kind), LinkPhase::Off | LinkPhase::Scanning) => LinkPhase::Failed(kind),
            (_, link) => link,
        }
    }
}
