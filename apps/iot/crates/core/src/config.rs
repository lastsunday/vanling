//! Acquisition configuration: *how* a part is wired to produce what a demand asks for.
//!
//! A configuration is deliberately not part of a [`crate::demand`]. A page asks for a
//! [`Mode`] and a period; the driver picks the registers, the transport
//! and the on-chip engines that meet it. Naming a configuration is the probe's business, and
//! its cost is measured there (`demand-probe`) rather than derived here.
//!
//! Whether a driver can actually leave a part in a configuration is a property of that
//! part's register table, decided where the table is open. Naming one here is a claim that
//! the shape is worth a word, not that any given board can enter it — the refusals are
//! listed at the driver.

use crate::demand::{EffectiveDemand, Mode, PlaneKey};

/// What the parts must be in for a demand to be met.
///
/// One field per plane that has an acquisition choice at all, because a plane with none
/// names nothing here: an empty entry would be a claim that the plane is configured, and
/// only the driver knows whether it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Configuration {
    /// `None` when the demand needs no motion configuration, which is `Mode::Off`.
    pub motion: Option<MotionConfig>,
}

/// Turns a demand into the configurations its parts must be in.
///
/// The whole of the translation, and the only place a mode reaches a register table: a page
/// asks what it wants in time, and this is where that becomes something a part can be told
/// to do. What the parts offer is supplied at construction because only the board knows its
/// own silicon — the demand and the mapping are the product's, the availability is not.
pub struct Configurator {
    available: &'static [MotionConfig],
}

impl Configurator {
    pub const fn new(available: &'static [MotionConfig]) -> Self {
        Self { available }
    }

    /// What this demand needs each part in, or the first mode whose configuration the parts
    /// cannot be told to enter.
    ///
    /// A refusal names the mode as well as the configuration so a reader can tell which
    /// demand went unmet, rather than having to infer it from a configuration nobody applied.
    pub fn configuration_for(
        &self,
        demand: &EffectiveDemand,
    ) -> Result<Configuration, Unreachable> {
        let motion = match MotionConfig::for_mode(demand.plane(PlaneKey::MOTION).mode) {
            None => None,
            Some(config) if self.available.contains(&config) => Some(config),
            Some(config) => {
                return Err(Unreachable {
                    key: PlaneKey::MOTION,
                    mode: demand.plane(PlaneKey::MOTION).mode,
                    config,
                });
            }
        };
        Ok(Configuration { motion })
    }
}

/// How the motion part is set up. Which of these a board can reach, and why the rest stay
/// out of reach, belongs to the driver holding the register table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MotionConfig {
    /// The accelerometer alone, armed as a "did something move" wake detector.
    WakeDetector,
    /// Both sensors read straight from their registers each poll.
    PolledRegisters,
    /// The part's FIFO buffering samples between polls.
    Fifo,
    /// Registers read each poll, with the on-chip low-pass filters armed.
    FilteredRegisters,
}

impl MotionConfig {
    /// What produces what `mode` asks for, in time. `None` means the mode needs no
    /// configuration, which is only `Off`: it is met by not collecting.
    ///
    /// Whether the part offers the answer is not decided here. A configurator that finds
    /// this configuration absent must refuse rather than substitute, and that check belongs
    /// to whoever holds the part's capabilities.
    pub const fn for_mode(mode: Mode) -> Option<Self> {
        match mode {
            Mode::Off => None,
            // One configuration meets both collecting modes, because the No-Motion engine
            // armed inside it is what turns a polled stream into change events. A wake
            // detector would buy power at the polling rate, not correctness, so naming it
            // here would put a requirement into the demand that the demand never made.
            Mode::OnChange | Mode::Continuous => Some(Self::PolledRegisters),
        }
    }
}

/// A mode this part has no encoding for.
///
/// Not a failure to try: the silicon has nowhere to put it. Answering with a configuration
/// that is merely similar is the silent downgrade this type exists to prevent, so a demand
/// the part cannot meet arrives here instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreachable {
    /// Which plane went unmet, so the demand can be amended rather than left claiming it.
    pub key: PlaneKey,
    pub mode: Mode,
    pub config: MotionConfig,
}

impl core::fmt::Display for Unreachable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:?} needs {config:?}, which this part cannot enter",
            self.mode,
            config = self.config
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demand::PlaneKey;
    use crate::demand::{PlaneDemand, Sensitivity};

    #[test]
    fn off_is_met_by_not_collecting_rather_than_by_a_configuration() {
        assert_eq!(MotionConfig::for_mode(Mode::Off), None);
    }

    #[test]
    fn both_collecting_modes_are_met_by_the_same_wiring() {
        // The No-Motion engine is armed inside PolledRegisters, so a polled stream already
        // produces change events. Were these two configurations apart, this assertion is
        // what would notice.
        assert_eq!(
            MotionConfig::for_mode(Mode::OnChange),
            MotionConfig::for_mode(Mode::Continuous)
        );
    }

    #[test]
    fn no_mode_reaches_a_configuration_the_recognizer_did_not_see() {
        // FilteredRegisters is a different stream from the one the core recognizer's bars
        // were calibrated against. Selecting it from a mode would be invisible: the demand
        // would be met and the thresholds would quietly stop meaning anything.
        for mode in [Mode::Off, Mode::OnChange, Mode::Continuous] {
            assert_ne!(
                MotionConfig::for_mode(mode),
                Some(MotionConfig::FilteredRegisters)
            );
        }
    }

    #[test]
    fn the_configuration_a_mode_needs_is_named_in_its_refusal() {
        let refusal = Unreachable {
            key: PlaneKey::MOTION,
            mode: Mode::OnChange,
            config: MotionConfig::WakeDetector,
        };
        assert!(
            refusal.to_string().contains("WakeDetector"),
            "a refusal a reader cannot act on is not a refusal: {refusal}"
        );
    }

    fn demanding(mode: Mode) -> EffectiveDemand {
        let mut demand = EffectiveDemand::none();
        demand.set(
            PlaneKey::MOTION,
            PlaneDemand::new(mode, 0, Sensitivity::Secondary),
        );
        demand
    }

    #[test]
    fn a_plane_the_part_can_reach_is_configured_rather_than_refused() {
        let configurator = Configurator::new(&[MotionConfig::PolledRegisters]);
        assert_eq!(
            configurator.configuration_for(&demanding(Mode::Off)),
            Ok(Configuration { motion: None })
        );
        for mode in [Mode::OnChange, Mode::Continuous] {
            assert_eq!(
                configurator.configuration_for(&demanding(mode)),
                Ok(Configuration {
                    motion: Some(MotionConfig::PolledRegisters)
                })
            );
        }
    }

    #[test]
    fn a_board_that_lacks_the_configuration_a_mode_needs_refuses_it() {
        let configurator = Configurator::new(&[]);
        let refusal = configurator
            .configuration_for(&demanding(Mode::Continuous))
            .unwrap_err();

        assert_eq!(
            refusal,
            Unreachable {
                key: PlaneKey::MOTION,
                mode: Mode::Continuous,
                config: MotionConfig::PolledRegisters,
            },
            "a refusal that does not name both the mode and the configuration leaves the \
             reader guessing which of them was missing"
        );
    }

    #[test]
    fn the_configurator_reads_nothing_about_the_part_beyond_what_it_was_given() {
        // The mapping is the product's, the availability is the board's. A configurator that
        // grew its own idea of what exists would stop being told when a board changed.
        assert_eq!(
            Configurator::new(&[
                MotionConfig::PolledRegisters,
                MotionConfig::FilteredRegisters
            ])
            .configuration_for(&demanding(Mode::OnChange)),
            Configurator::new(&[MotionConfig::PolledRegisters])
                .configuration_for(&demanding(Mode::OnChange))
        );
    }
}
