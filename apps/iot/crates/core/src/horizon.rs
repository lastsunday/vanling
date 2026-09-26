//! Attitude-dial geometry: rolls/pitches to a normalized horizon line,
//! keeping sign conventions host-testable so the panel layer only stamps.

use core::f32::consts::PI;
use libm::{cosf, roundf, sinf};

/// Fixed-point scale of the normalized geometry: `1_000` = one dial radius.
pub const SCALE: i32 = 1_000;

/// The horizon line, normalized to the dial radius (`1_000` units) so any
/// panel just multiplies by its own radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Horizon {
    /// Unit direction along the line (`cos(-roll)`, `sin(-roll)`) so the
    /// horizon stays level with the world while the dial frame rolls.
    pub unit: (i32, i32),
    /// Pitch shift along the vertical axis (one radius = `1_000`); a nose-up
    /// pitch moves the line toward the dial's bottom edge.
    pub offset: i32,
}

/// Builds the horizon from `tilt_deg_x10` (`roll`, `pitch` in tenths of a
/// degree): slope `-roll`, vertical shift `sin(pitch)`.
pub fn horizon(roll_deg_x10: i16, pitch_deg_x10: i16) -> Horizon {
    let roll = f32::from(roll_deg_x10) / 10.0 * PI / 180.0;
    let unit = (
        roundf(cosf(-roll) * SCALE as f32),
        roundf(sinf(-roll) * SCALE as f32),
    );
    let pitch = f32::from(pitch_deg_x10) / 10.0 * PI / 180.0;
    let offset = roundf(sinf(pitch) * SCALE as f32);
    Horizon {
        unit: (unit.0 as i32, unit.1 as i32),
        offset: offset as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenths(deg: i16) -> i16 {
        deg * 10
    }

    #[test]
    fn level_reads_horizontal_and_centered() {
        let h = horizon(0, 0);
        assert_eq!(h.unit, (1_000, 0));
        assert_eq!(h.offset, 0);
    }

    #[test]
    fn right_roll_tilts_horizon_up_on_the_right() {
        let h = horizon(tenths(90), 0);
        assert_eq!(h.unit, (0, -1_000));
    }

    #[test]
    fn left_roll_tilts_horizon_up_on_the_left() {
        let h = horizon(tenths(-90), 0);
        assert_eq!(h.unit, (0, 1_000));
    }

    #[test]
    fn nose_up_displaces_horizon_downward() {
        assert_eq!(horizon(0, tenths(30)).offset, 500);
        assert_eq!(horizon(0, tenths(90)).offset, 1_000);
    }

    #[test]
    fn nose_down_displaces_horizon_upward() {
        assert_eq!(horizon(0, tenths(-30)).offset, -500);
        assert_eq!(horizon(0, tenths(-90)).offset, -1_000);
    }

    #[test]
    fn geometry_stays_within_one_radius() {
        for roll in (-180..=180).step_by(10) {
            for pitch in (-180..=180).step_by(10) {
                let h = horizon(tenths(roll), tenths(pitch));
                assert!(h.unit.0.abs() <= SCALE, "unit x out of dial: {roll}");
                assert!(h.unit.1.abs() <= SCALE, "unit y out of dial: {roll}");
                assert!(h.offset.abs() <= SCALE, "offset out of dial: {pitch}");
            }
        }
    }
}
