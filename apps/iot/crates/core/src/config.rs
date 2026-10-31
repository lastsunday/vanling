//! Acquisition configuration: *how* a part is wired to produce what a demand asks for.

use crate::demand::{DemandKey, EffectiveDemand, Mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Configuration {
    /// `None` when the demand needs no motion configuration, which is `Mode::Off`.
    pub motion: Option<MotionConfig>,
}

/// Turns a demand into the configurations its parts must be in.
pub struct Configurator {
    available: &'static [MotionConfig],
}

impl Configurator {
    pub const fn new(available: &'static [MotionConfig]) -> Self {
        Self { available }
    }

    /// What this demand needs each part in, or the first mode whose configuration the parts
    /// cannot be told to enter.
    pub fn configuration_for(
        &self,
        demand: &EffectiveDemand,
    ) -> Result<Configuration, Unreachable> {
        let motion = match MotionConfig::for_mode(demand.claim(DemandKey::MOTION).mode) {
            None => None,
            Some(config) if self.available.contains(&config) => Some(config),
            Some(config) => {
                return Err(Unreachable {
                    key: DemandKey::MOTION,
                    mode: demand.claim(DemandKey::MOTION).mode,
                    config,
                });
            }
        };
        Ok(Configuration { motion })
    }
}

/// How the motion part is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MotionConfig {
    WakeDetector,
    PolledRegisters,
    Fifo,
    FilteredRegisters,
}

impl MotionConfig {
    /// What produces what `mode` asks for, in time.
    pub const fn for_mode(mode: Mode) -> Option<Self> {
        match mode {
            Mode::Off => None,
            Mode::OnChange | Mode::Continuous => Some(Self::PolledRegisters),
        }
    }
}

/// A mode this part has no encoding for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreachable {
    pub key: DemandKey,
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
    use crate::demand::DemandKey;
    use crate::demand::{Claim, Priority};

    #[test]
    fn off_is_met_by_not_collecting_rather_than_by_a_configuration() {
        assert_eq!(MotionConfig::for_mode(Mode::Off), None);
    }

    #[test]
    fn both_collecting_modes_are_met_by_the_same_wiring() {
        assert_eq!(
            MotionConfig::for_mode(Mode::OnChange),
            MotionConfig::for_mode(Mode::Continuous)
        );
    }

    #[test]
    fn no_mode_reaches_a_configuration_the_recognizer_did_not_see() {
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
            key: DemandKey::MOTION,
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
        demand.set(DemandKey::MOTION, Claim::new(mode, 0, Priority::Secondary));
        demand
    }

    #[test]
    fn a_device_the_part_can_reach_is_configured_rather_than_refused() {
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
                key: DemandKey::MOTION,
                mode: Mode::Continuous,
                config: MotionConfig::PolledRegisters,
            },
            "a refusal that does not name both the mode and the configuration leaves the \
             reader guessing which of them was missing"
        );
    }

    #[test]
    fn the_configurator_reads_nothing_about_the_part_beyond_what_it_was_given() {
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
