//! Colour primitives and the maths that shapes them.
//!
//! Shared by the drivers that show a colour and by the pages that draw on one: nothing here
//! knows which of the two is asking.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Max colour waypoints a `group` array can carry; unused slots pad with `0`.
pub const GROUP_CAPACITY: usize = 7;

/// Light-mode codes carried by a light snapshot: `0` off, `1` breathing, `2` solid.
pub const MODE_OFF: u8 = 0;
pub const MODE_BREATH: u8 = 1;
pub const MODE_SOLID: u8 = 2;

/// How a light moves. Spatial effects need more than one colour, so a single surface has these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Effect {
    Steady,
    #[default]
    Breath,
    Cycle,
    Aurora,
    Candle,
    Pulse,
    Heartbeat,
    Lightning,
    Sparkle,
    Strobe,
    Police,
    Sos,
}

/// The parameters every effect shares: an envelope, a hue path and a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectParams {
    pub period_ms: u32,
    pub hue_period_ms: u32,
    pub hue_span: u8,
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub saturation: u8,
    pub hue: u8,
    pub group: [u8; GROUP_CAPACITY],
    pub group_len: u8,
}

const FIXED_POINT: u64 = 1 << 16;

/// Smooth sin² brightness envelope scaled to `[min_brightness, max_brightness]`.
pub fn smooth_brightness(
    elapsed_ms: u32,
    period_ms: u32,
    min_brightness: u8,
    max_brightness: u8,
) -> u8 {
    let min = u64::from(min_brightness.min(max_brightness));
    let scale = u64::from(max_brightness) - min;
    if scale == 0 {
        return min as u8;
    }

    let period = u64::from(period_ms.max(1));
    let phase = u64::from(elapsed_ms) % period;
    let phase_fraction = phase * FIXED_POINT / period;

    (min + scale * hann_weight(phase_fraction) / FIXED_POINT) as u8
}

fn hann_weight(phase_fraction: u64) -> u64 {
    let parabola = phase_fraction * (FIXED_POINT - phase_fraction) / FIXED_POINT;
    let bhaskara_sin = 16 * FIXED_POINT * parabola / (5 * FIXED_POINT - 4 * parabola);
    bhaskara_sin * bhaskara_sin / FIXED_POINT
}

/// Hue on a 256-step wheel, advancing linearly and wrapping every `period_ms`.
pub fn hue_phase(elapsed_ms: u32, period_ms: u32) -> u8 {
    let period = u64::from(period_ms.max(1));
    let phase = u64::from(elapsed_ms) % period;
    let hue_tick = phase * 256 / period;
    hue_tick as u8
}

/// Convert HSV to RGB (256-step hue, 8-bit saturation/value).
pub fn hsv_to_rgb(hue: u8, saturation: u8, value: u8) -> Rgb {
    let sat = u32::from(saturation);
    let val = u32::from(value);
    let x = u32::from(hue) * 6;
    let region = x / 256;
    let remainder = x % 256;

    let floor = val * (255 - sat) / 255;
    let ascending = val * (255 - sat * (255 - remainder) / 256) / 255;
    let descending = val * (255 - sat * remainder / 256) / 255;

    match region {
        0 => Rgb(val as u8, ascending as u8, floor as u8),
        1 => Rgb(descending as u8, val as u8, floor as u8),
        2 => Rgb(floor as u8, val as u8, ascending as u8),
        3 => Rgb(floor as u8, descending as u8, val as u8),
        4 => Rgb(ascending as u8, floor as u8, val as u8),
        _ => Rgb(val as u8, floor as u8, descending as u8),
    }
}

/// Hue of an RGB color on the same 256-step wheel `hsv_to_rgb` consumes, for a debug
/// readout of a solid color.
pub fn rgb_hue(Rgb(r, g, b): Rgb) -> u8 {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let span = max - min;
    if span == 0 {
        return 0;
    }
    let raw = if max == r {
        if g >= b { g - b } else { g - b + 6 * span }
    } else if max == g {
        b - r + 2 * span
    } else {
        r - g + 4 * span
    };
    let hue = (raw * 256 * 2 + 6 * span) / (12 * span);
    if hue > 255 { 255 } else { hue as u8 }
}

/// The brightest channel: how much a colour is on, independent of its hue.
pub fn brightness_of(Rgb(r, g, b): Rgb) -> u8 {
    r.max(g).max(b)
}

/// Scale every `color` channel toward black by `brightness`/255.
pub fn scale_brightness(color: Rgb, brightness: u8) -> Rgb {
    let scale = u16::from(brightness);
    let scale_channel = |c: u8| (u16::from(c) * scale / 255) as u8;
    Rgb(
        scale_channel(color.0),
        scale_channel(color.1),
        scale_channel(color.2),
    )
}

/// One frame of a breathing schedule: the colour it paints, and where in the brightness
/// envelope that frame sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub color: Rgb,
    /// `0` at the trough, `255` at the peak; a flat envelope samples 255.
    pub phase: u8,
}

impl EffectParams {
    /// One frame of `effect` at `elapsed_ms`.
    pub fn sample(self, effect: Effect, elapsed_ms: u32) -> Sample {
        let max = self.max_brightness;
        let min = self.min_brightness.min(max);
        let (value, hue) = match effect {
            Effect::Steady => (max, self.hue_at(0)),
            Effect::Breath => (
                smooth_brightness(elapsed_ms, self.period_ms, min, max),
                self.hue_at(elapsed_ms),
            ),
            Effect::Cycle => (
                max,
                group_hue(
                    elapsed_ms,
                    self.period_ms,
                    255,
                    self.hue,
                    [0; GROUP_CAPACITY],
                    0,
                ),
            ),
            Effect::Aurora => {
                let (group, len) = if self.group_len >= 2 {
                    (self.group, self.group_len)
                } else {
                    aurora_group(self.hue)
                };
                let period = if self.hue_period_ms == 0 {
                    self.period_ms.saturating_mul(4)
                } else {
                    self.hue_period_ms
                };
                (
                    smooth_brightness(elapsed_ms, self.period_ms, min, max),
                    group_hue(elapsed_ms, period, 0, self.hue, group, len),
                )
            }
            Effect::Candle => {
                let (value, jitter) = candle(elapsed_ms, self.period_ms, min, max);
                (value, self.hue.wrapping_add(jitter as u8))
            }
            Effect::Pulse => (
                pulse(elapsed_ms, self.period_ms, min, max),
                self.hue_at(elapsed_ms),
            ),
            Effect::Heartbeat => (
                heartbeat(elapsed_ms, self.period_ms, min, max),
                self.hue_at(elapsed_ms),
            ),
            Effect::Lightning => (
                lightning(elapsed_ms, self.period_ms, min, max),
                self.hue_at(elapsed_ms),
            ),
            Effect::Sparkle => (
                sparkle(elapsed_ms, self.period_ms, min, max),
                self.hue_at(elapsed_ms),
            ),
            Effect::Strobe => (
                strobe(elapsed_ms, self.period_ms, min, max),
                self.hue_at(elapsed_ms),
            ),
            Effect::Police => (
                max,
                police_hue(elapsed_ms, self.period_ms, self.group, self.group_len),
            ),
            Effect::Sos => (
                sos(elapsed_ms, self.period_ms, min, max),
                self.hue_at(elapsed_ms),
            ),
        };
        let span = max.saturating_sub(min);
        let phase = match span {
            0 => u8::MAX,
            _ => (u16::from(value.saturating_sub(min)) * 255 / u16::from(span)) as u8,
        };
        Sample {
            color: hsv_to_rgb(hue, self.saturation, value),
            phase,
        }
    }

    fn hue_at(self, elapsed_ms: u32) -> u8 {
        group_hue(
            elapsed_ms,
            self.hue_period_ms,
            self.hue_span,
            self.hue,
            self.group,
            self.group_len,
        )
    }
}

/// Soft color-graduated breathing at `elapsed_ms`, hue path per `group_len` (see
/// `group_hue`); `hue_span == 0` holds `base_hue` static — standalone is a parameter value,
/// not a branch.
#[allow(clippy::too_many_arguments)]
pub fn sample(
    elapsed_ms: u32,
    breath_period_ms: u32,
    hue_period_ms: u32,
    hue_span: u8,
    min_brightness: u8,
    max_brightness: u8,
    saturation: u8,
    base_hue: u8,
    group: [u8; GROUP_CAPACITY],
    group_len: u8,
) -> Sample {
    EffectParams {
        period_ms: breath_period_ms,
        hue_period_ms,
        hue_span,
        min_brightness,
        max_brightness,
        saturation,
        hue: base_hue,
        group,
        group_len,
    }
    .sample(Effect::Breath, elapsed_ms)
}

/// Aurora's four waypoints, spread enough to read as green→cyan→blue→violet.
const AURORA_OFFSETS: [u8; 4] = [0, 40, 80, 140];

fn aurora_group(base: u8) -> ([u8; GROUP_CAPACITY], u8) {
    let mut group = [0; GROUP_CAPACITY];
    for (slot, offset) in group.iter_mut().zip(AURORA_OFFSETS) {
        *slot = base.wrapping_add(offset);
    }
    (group, AURORA_OFFSETS.len() as u8)
}

/// A small, non-repeating-looking flicker: two Hann waves of different periods, kept near
/// the peak so text over the glow stays readable. Hue jitter follows the faster wave.
fn candle(elapsed_ms: u32, period_ms: u32, min: u8, max: u8) -> (u8, i8) {
    if max == 0 {
        return (0, 0);
    }
    let period = period_ms.max(1);
    let slow = u16::from(smooth_brightness(elapsed_ms, period, 0, 255));
    let fast_period = (period.saturating_mul(2) / 3).max(1);
    let fast = u16::from(smooth_brightness(
        elapsed_ms.wrapping_add(period / 3),
        fast_period,
        0,
        255,
    ));
    let mix = (slow * 2 + fast) / 3;
    let amplitude = ((u16::from(max) - u16::from(min)) / 6).max(1);
    let floor = u16::from(max).saturating_sub(amplitude);
    let value = floor + amplitude * mix / 255;
    let jitter = ((i32::from(fast) - i32::from(slow)) * 4 / 255) as i8;
    (value as u8, jitter)
}

/// Two beats per period, separated by a still gap.
fn pulse(elapsed_ms: u32, period_ms: u32, min: u8, max: u8) -> u8 {
    let period = period_ms.max(1);
    let beat = (period / 6).max(1);
    let t = u64::from(elapsed_ms % period);
    let weight =
        bump(t, 0, u64::from(beat) * 2).max(bump(t, u64::from(beat) * 2, u64::from(beat) * 2));
    let floor = u32::from(min.min(max));
    let scale = u32::from(max.max(min)) - floor;
    (floor + scale * weight as u32 / FIXED_POINT as u32) as u8
}

fn bump(t: u64, start: u64, len: u64) -> u64 {
    if t < start || t >= start + len {
        return 0;
    }
    hann_weight((t - start) * FIXED_POINT / len)
}

/// A deterministic 32-bit mixer: the same counter always yields the same noise on every
/// target, which keeps randomized-looking effects testable.
const fn mix(mut x: u32) -> u32 {
    x = x.wrapping_add(0x9e37_79b9);
    x = (x ^ (x >> 16)).wrapping_mul(0x21f0_aaad);
    x = (x ^ (x >> 15)).wrapping_mul(0x735a_2d97);
    x ^ (x >> 15)
}

/// `min..=max` at `weight`, where `FIXED_POINT` is full scale.
fn lerp_brightness(min: u8, max: u8, weight: u64) -> u8 {
    let floor = u32::from(min.min(max));
    let scale = u32::from(max.max(min)) - floor;
    (floor + scale * (weight.min(FIXED_POINT)) as u32 / FIXED_POINT as u32) as u8
}

/// A lub-dub: a strong beat, a closer weaker one, then a long rest.
fn heartbeat(elapsed_ms: u32, period_ms: u32, min: u8, max: u8) -> u8 {
    let period = u64::from(period_ms.max(1));
    let t = u64::from(elapsed_ms) % period;
    let lub = (period / 7).max(1);
    let dub = (period / 10).max(1);
    let dub_start = lub + (period / 14).max(1);
    let weight = bump(t, 0, lub).max(bump(t, dub_start, dub) * 3 / 4);
    lerp_brightness(min, max, weight)
}

/// One to three short flashes per cycle, chosen per cycle so the pattern varies but repeats
/// exactly.
fn lightning(elapsed_ms: u32, period_ms: u32, min: u8, max: u8) -> u8 {
    const SLOTS: u64 = 8;
    let period = u64::from(period_ms.max(1));
    let t = u64::from(elapsed_ms) % period;
    let noise = mix((u64::from(elapsed_ms) / period) as u32);
    let slot = (period / SLOTS).max(1);
    let count = 1 + u64::from(noise % 3);
    let first = u64::from((noise >> 5) % SLOTS as u32);
    let mut weight = 0;
    for i in 0..count {
        let start = ((first + i * 2) % SLOTS) * slot;
        let len = (slot / 2).max(1);
        let strength = if (noise >> (8 + i as u32)) & 1 == 0 {
            FIXED_POINT
        } else {
            FIXED_POINT * 3 / 4
        };
        weight = weight.max(bump(t, start, len) * strength / FIXED_POINT);
    }
    lerp_brightness(min, max, weight)
}

/// A handful of short twinkles: each eighth of the period may spark, then decays to the floor.
fn sparkle(elapsed_ms: u32, period_ms: u32, min: u8, max: u8) -> u8 {
    const SLOTS: u64 = 8;
    let period = u64::from(period_ms.max(1));
    let t = u64::from(elapsed_ms) % period;
    let cycle = (u64::from(elapsed_ms) / period) as u32;
    let slot = (period / SLOTS).max(1);
    let noise = mix(cycle
        .wrapping_mul(SLOTS as u32)
        .wrapping_add((t / slot) as u32));
    if !noise.is_multiple_of(4) {
        return min.min(max);
    }
    let decay = FIXED_POINT - (t % slot) * FIXED_POINT / slot;
    let strength = 128 + u64::from((noise >> 8) % 128);
    lerp_brightness(min, max, decay * strength / 255)
}

/// A hard square wave: the first eighth of the period is on, the rest is off.
fn strobe(elapsed_ms: u32, period_ms: u32, min: u8, max: u8) -> u8 {
    let period = u64::from(period_ms.max(1));
    if u64::from(elapsed_ms) % period < (period / 8).max(1) {
        max.max(min)
    } else {
        min.min(max)
    }
}

const POLICE_GROUP: [u8; GROUP_CAPACITY] = [0, 170, 0, 0, 0, 0, 0];

/// Two hues half a period apart, cut hard rather than interpolated.
fn police_hue(elapsed_ms: u32, period_ms: u32, group: [u8; GROUP_CAPACITY], group_len: u8) -> u8 {
    let period = u64::from(period_ms.max(1));
    let half = (period / 2).max(1);
    let index = ((u64::from(elapsed_ms) / half) % 2) as usize;
    let hues = if group_len == 2 { group } else { POLICE_GROUP };
    hues[index]
}

/// Morse SOS at one unit per thirty-sixth of the period, with a long message gap.
fn sos(elapsed_ms: u32, period_ms: u32, min: u8, max: u8) -> u8 {
    const STEPS: [(bool, u8); 18] = [
        (true, 1),
        (false, 1),
        (true, 1),
        (false, 1),
        (true, 1),
        (false, 3),
        (true, 3),
        (false, 1),
        (true, 3),
        (false, 1),
        (true, 3),
        (false, 3),
        (true, 1),
        (false, 1),
        (true, 1),
        (false, 1),
        (true, 1),
        (false, 9),
    ];
    let unit = (period_ms.max(1) / 36).max(1);
    let total: u64 = STEPS
        .iter()
        .map(|(_, units)| u64::from(*units) * u64::from(unit))
        .sum();
    let t = u64::from(elapsed_ms) % total.max(1);
    let mut at = 0;
    for (on, units) in STEPS {
        let len = u64::from(units) * u64::from(unit);
        if t < at + len {
            return if on { max.max(min) } else { min.min(max) };
        }
        at += len;
    }
    min.min(max)
}

/// Hue at `elapsed_ms` along the configured path: `group_len == 0` walks the
/// `hue`/`hue_span` sweep (`0` span holds `base_hue`); `group_len == 1` holds `group[0]`;
/// `group_len >= 2` rotates through the waypoints, walking each adjacent pair over
/// `hue_period_ms / group_len` and wrapping the last back to the first.
pub fn group_hue(
    elapsed_ms: u32,
    hue_period_ms: u32,
    hue_span: u8,
    base_hue: u8,
    group: [u8; GROUP_CAPACITY],
    group_len: u8,
) -> u8 {
    match group_len {
        0 => u8::wrapping_add(base_hue, hue_offset(hue_span, hue_period_ms, elapsed_ms)),
        1 => group[0],
        _ => {
            let leg_ms = (hue_period_ms / u32::from(group_len)).max(1);
            let cycle_ms = leg_ms * u32::from(group_len);
            let t = elapsed_ms % cycle_ms;
            let waypoint = (t / leg_ms) as usize;
            let segment_ms = t % leg_ms;
            let fraction = u64::from(segment_ms) * 256 / u64::from(leg_ms);
            let from = i32::from(group[waypoint]);
            let to = i32::from(group[(waypoint + 1) % usize::from(group_len)]);
            let delta = ((to - from + 384) % 256) - 128;
            let hue = from + delta * (fraction as i32) / 256;
            hue.rem_euclid(256) as u8
        }
    }
}

fn hue_offset(hue_span: u8, hue_period_ms: u32, elapsed_ms: u32) -> u8 {
    if hue_span == 0 || hue_period_ms == 0 {
        return 0;
    }
    (u32::from(hue_span) * u32::from(hue_phase(elapsed_ms, hue_period_ms)) / 256) as u8
}

/// Packs a colour into the panel's big-endian RGB565 word.
pub fn rgb565(color: Rgb) -> u16 {
    let r = color.0 as u16;
    let g = color.1 as u16;
    let b = color.2 as u16;
    ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
}

/// Per-channel bitwise inversion of an RGB565 word: a glyph drawn this way is visible on any
/// fill, matching the "off" side of the contrast regardless of base colour.
pub fn invert_rgb565(packed: u16) -> u16 {
    let r = (packed >> 11) & 0x1F;
    let g = (packed >> 5) & 0x3F;
    let b = packed & 0x1F;
    ((0x1F - r) << 11) | ((0x3F - g) << 5) | (0x1F - b)
}

#[cfg(test)]
mod tests {
    /// The colour half of [`sample`], so the envelope's tests read as colour comparisons.
    #[allow(clippy::too_many_arguments)]
    fn breathe(
        elapsed_ms: u32,
        period_ms: u32,
        hue_period_ms: u32,
        hue_span: u8,
        min_brightness: u8,
        max_brightness: u8,
        saturation: u8,
        hue: u8,
        group: [u8; GROUP_CAPACITY],
        group_len: u8,
    ) -> Rgb {
        sample(
            elapsed_ms,
            period_ms,
            hue_period_ms,
            hue_span,
            min_brightness,
            max_brightness,
            saturation,
            hue,
            group,
            group_len,
        )
        .color
    }

    use super::*;

    #[test]
    fn smooth_brightness_shapes_a_soft_arc() {
        // The trough and peak of a full 2000ms period.
        assert_eq!(smooth_brightness(0, 2000, 0, 255), 0);
        assert_eq!(smooth_brightness(2000, 2000, 0, 255), 0);
        assert_eq!(smooth_brightness(1000, 2000, 0, 255), 255);
        assert_eq!(smooth_brightness(1000, 2000, 0, 80), 80);
        assert_eq!(
            smooth_brightness(500, 2000, 0, 255),
            smooth_brightness(1500, 2000, 0, 255)
        );
        let rising: Vec<u8> = (0..=1000)
            .step_by(100)
            .map(|t| smooth_brightness(t, 2000, 0, 255))
            .collect();
        for w in rising.windows(2) {
            assert!(w[0] <= w[1], "not monotonic rising: {:?}", rising);
        }
        let falling: Vec<u8> = (1000..=2000)
            .step_by(100)
            .map(|t| smooth_brightness(t, 2000, 0, 255))
            .collect();
        for w in falling.windows(2) {
            assert!(w[0] >= w[1], "not monotonic falling: {:?}", falling);
        }
        for t in (0..=2000).step_by(13) {
            let v = smooth_brightness(t, 2000, 0, 80);
            assert!(v <= 80, "t={t}: {v} > 80");
        }
        assert!(smooth_brightness(900, 2000, 0, 255) > smooth_brightness(500, 2000, 0, 255) / 2);
        assert!(smooth_brightness(100, 2000, 0, 255) < 16);
        assert!(smooth_brightness(1900, 2000, 0, 255) < 16);
        // Degenerate inputs never panic; a nonzero floor keeps the trough lit.
        assert_eq!(smooth_brightness(0, 0, 0, 255), 0);
        assert_eq!(smooth_brightness(123, 0, 0, 255), 0);
        assert_eq!(smooth_brightness(123, 2000, 0, 0), 0);
        assert_eq!(smooth_brightness(0, 2000, 24, 80), 24);
        assert_eq!(smooth_brightness(2000, 2000, 24, 80), 24);
        assert_eq!(smooth_brightness(1000, 2000, 24, 80), 80);
    }

    #[test]
    fn hue_phase_advances_and_wraps() {
        assert_eq!(hue_phase(0, 8000), 0);
        assert_eq!(hue_phase(2000, 8000), 64);
        assert_eq!(hue_phase(4000, 8000), 128);
        assert_eq!(hue_phase(7999, 8000), 255);
        assert_eq!(hue_phase(8000, 8000), 0);
        assert_eq!(hue_phase(123, 0), 0, "a zero period stays at the base hue");
    }

    #[test]
    fn hsv_maps_primaries_secondaries_and_achromatic() {
        let red = hsv_to_rgb(0, 255, 255);
        assert_eq!(red.0, 255);
        assert!(red.1 <= 2);
        assert_eq!(red.2, 0);
        let green = hsv_to_rgb(85, 255, 255);
        assert!(green.0 <= 2);
        assert_eq!(green.1, 255);
        assert_eq!(green.2, 0);
        let blue = hsv_to_rgb(170, 255, 255);
        assert!(blue.0 <= 2);
        assert!(blue.1 <= 4);
        assert_eq!(blue.2, 255);
        let tail = hsv_to_rgb(255, 255, 255);
        assert_eq!(tail.0, 255);
        assert!(tail.1 <= 2);
        assert!(tail.2 <= 8);
        assert_eq!(hsv_to_rgb(37, 0, 200), Rgb(200, 200, 200));
        assert_eq!(hsv_to_rgb(200, 0, 80), Rgb(80, 80, 80));
        assert_eq!(hsv_to_rgb(100, 255, 0), Rgb(0, 0, 0));
    }

    #[test]
    fn rgb_hue_derives_the_same_wheel_back() {
        for hue in [0u8, 16, 43, 85, 128, 170, 213, 255] {
            let rgb = hsv_to_rgb(hue, 255, 255);
            let back = rgb_hue(rgb);
            assert!(
                u8::abs_diff(back, hue) <= 1,
                "hue {hue} -> {:?} -> {back}",
                rgb
            );
        }
        assert_eq!(rgb_hue(Rgb(255, 255, 0)), 43); // yellow
        assert_eq!(rgb_hue(Rgb(0, 255, 255)), 128); // cyan
        assert_eq!(rgb_hue(Rgb(255, 0, 255)), 213); // magenta
        assert_eq!(rgb_hue(Rgb(120, 120, 120)), 0);
        assert_eq!(rgb_hue(Rgb(0, 0, 0)), 0);
    }

    #[test]
    fn breathe_drives_the_hann_envelope() {
        // At t=0 the envelope is dark; a zero period never panics.
        assert_eq!(
            breathe(0, 2000, 8000, 255, 0, 80, 200, 0, [0, 0, 0, 0, 0, 0, 0], 0),
            Rgb(0, 0, 0)
        );
        assert_eq!(
            breathe(123, 0, 0, 255, 0, 80, 200, 0, [0, 0, 0, 0, 0, 0, 0], 0),
            Rgb(0, 0, 0)
        );
        let color = breathe(
            1000,
            2000,
            8000,
            255,
            0,
            128,
            255,
            0,
            [0, 0, 0, 0, 0, 0, 0],
            0,
        );
        assert_eq!(color.0, 128);
        assert!(color.1 > 0 && color.1 < 128);
        assert_eq!(color.2, 0);
        let rising = breathe(
            500,
            2000,
            8000,
            0,
            0,
            128,
            255,
            170,
            [0, 0, 0, 0, 0, 0, 0],
            0,
        );
        let falling = breathe(
            1500,
            2000,
            8000,
            0,
            0,
            128,
            255,
            170,
            [0, 0, 0, 0, 0, 0, 0],
            0,
        );
        assert_eq!(rising, falling);
        assert!(rising.2 > rising.0, "blue dominates at hue 170");
    }

    #[test]
    fn breathe_sweep_scales_with_span_and_saturation() {
        let color = breathe(
            0,
            2000,
            8000,
            100,
            0,
            128,
            255,
            42,
            [0, 0, 0, 0, 0, 0, 0],
            0,
        );
        let static_at_42 = breathe(0, 2000, 8000, 0, 0, 128, 255, 42, [0, 0, 0, 0, 0, 0, 0], 0);
        assert_eq!(color, static_at_42);
        let full = breathe(
            1000,
            2000,
            8000,
            255,
            0,
            128,
            255,
            0,
            [0, 0, 0, 0, 0, 0, 0],
            0,
        );
        let half = breathe(
            1000,
            2000,
            8000,
            128,
            0,
            128,
            255,
            0,
            [0, 0, 0, 0, 0, 0, 0],
            0,
        );
        assert_ne!(full, half);
        assert_eq!(full.0, 128);
        assert_eq!(half.0, 128);
        assert_eq!(full.2, 0);
        assert_eq!(half.2, 0);
        assert!(full.1 > half.1);
        let muted = breathe(1000, 2000, 8000, 0, 0, 128, 64, 0, [0, 0, 0, 0, 0, 0, 0], 0);
        assert_eq!(muted.0, 128);
        assert!(muted.1 > full.1, "lower saturation lifts the green channel");
        assert!(muted.2 > full.2, "lower saturation lifts the blue channel");
    }

    #[test]
    fn group_rotation_interpolates_wraps_and_degrades_gracefully() {
        // group [16, 32, 100]/3, hue_period 3000 → legs of 1000ms.
        // At the midpoint of the first leg the hue is halfway 16→32.
        assert_eq!(
            group_hue(500, 3000, 255, 0, [16, 32, 100, 0, 0, 0, 0], 3),
            24
        );
        let color = breathe(
            1000,
            2000,
            3000,
            255,
            0,
            128,
            255,
            0,
            [16, 32, 100, 0, 0, 0, 0],
            3,
        );
        assert_eq!(
            color,
            hsv_to_rgb(32, 255, smooth_brightness(1000, 2000, 0, 128))
        );
        assert_eq!(
            group_hue(2500, 3000, 255, 0, [16, 32, 100, 0, 0, 0, 0], 3),
            58
        );
        assert_eq!(
            group_hue(3000, 3000, 255, 0, [16, 32, 100, 0, 0, 0, 0], 3),
            16
        );
        assert_eq!(
            group_hue(125, 1000, 255, 0, [250, 16, 0, 0, 0, 0, 0], 2),
            255
        );
        assert_eq!(group_hue(250, 1000, 255, 0, [250, 16, 0, 0, 0, 0, 0], 2), 5);
        assert_eq!(
            group_hue(500, 1000, 255, 0, [250, 16, 0, 0, 0, 0, 0], 2),
            16
        );
        assert_eq!(group_hue(0, 8000, 255, 0, [170, 0, 0, 0, 0, 0, 0], 1), 170);
        assert_eq!(
            group_hue(7000, 8000, 255, 0, [170, 0, 0, 0, 0, 0, 0], 1),
            170
        );
        let swept = breathe(
            1234,
            2000,
            8000,
            128,
            0,
            128,
            200,
            42,
            [0, 0, 0, 0, 0, 0, 0],
            0,
        );
        let swept_hue = u8::wrapping_add(42, hue_offset(128, 8000, 1234));
        assert_eq!(
            swept,
            hsv_to_rgb(swept_hue, 200, smooth_brightness(1234, 2000, 0, 128))
        );
    }

    #[test]
    fn scale_brightness_fades_full_toward_black() {
        assert_eq!(scale_brightness(Rgb(200, 100, 50), 255), Rgb(200, 100, 50));
        assert_eq!(scale_brightness(Rgb(200, 100, 50), 128).0, 100);
        assert_eq!(scale_brightness(Rgb(100, 0, 0), 128).0, 50);
        assert_eq!(scale_brightness(Rgb(255, 255, 255), 0), Rgb(0, 0, 0));
    }

    #[test]
    fn a_sample_reports_its_place_in_the_envelope_as_well_as_its_colour() {
        let full = sample(0, 2000, 0, 0, 0, 255, 0, 0, [0; GROUP_CAPACITY], 0);
        assert_eq!(full.phase, 0, "the trough is the bottom of the envelope");
        assert_eq!(full.color, Rgb(0, 0, 0));
        let peak = sample(1000, 2000, 0, 0, 0, 255, 0, 0, [0; GROUP_CAPACITY], 0);
        assert_eq!(peak.phase, u8::MAX);
        assert_eq!(peak.color, Rgb(255, 255, 255));
        let flat = sample(0, 2000, 0, 0, 80, 80, 0, 0, [0; GROUP_CAPACITY], 0);
        assert_eq!(flat.phase, u8::MAX, "a flat envelope is at its own peak");
    }

    fn effect_params(period_ms: u32) -> EffectParams {
        EffectParams {
            period_ms,
            hue_period_ms: 0,
            hue_span: 0,
            min_brightness: 0,
            max_brightness: 200,
            saturation: 255,
            hue: 0,
            group: [0; GROUP_CAPACITY],
            group_len: 0,
        }
    }

    #[test]
    fn steady_holds_one_frame_at_the_peak() {
        let params = effect_params(1_000);
        let first = params.sample(Effect::Steady, 0);
        let later = params.sample(Effect::Steady, 777);
        assert_eq!(first, later);
        assert_eq!(first.phase, u8::MAX);
        assert_eq!(brightness_of(first.color), 200);
        let grouped = EffectParams {
            group: [40, 80, 0, 0, 0, 0, 0],
            group_len: 3,
            ..params
        };
        assert_eq!(
            grouped.sample(Effect::Steady, 0).color,
            hsv_to_rgb(40, 255, 200),
            "a steady light holds the group's head"
        );
    }

    #[test]
    fn cycle_holds_brightness_and_walks_the_hue_once_per_period() {
        let params = effect_params(8_000);
        let first = params.sample(Effect::Cycle, 0);
        let quarter = params.sample(Effect::Cycle, 2_000);
        let wrap = params.sample(Effect::Cycle, 8_000);
        assert_eq!(brightness_of(first.color), 200);
        assert_eq!(brightness_of(quarter.color), 200);
        // A 255-step sweep reaches hue 63 at the quarter (255 * 64 / 256).
        assert_eq!(quarter.color, hsv_to_rgb(63, 255, 200));
        assert_eq!(first.color, wrap.color, "the hue wraps at the period");
    }

    #[test]
    fn aurora_breathes_through_four_waypoints() {
        let mut params = effect_params(1_000);
        params.min_brightness = 40;
        let start = params.sample(Effect::Aurora, 0);
        let second = params.sample(Effect::Aurora, 1_000);
        assert_eq!(start.color, hsv_to_rgb(0, 255, 40));
        assert_eq!(
            second.color,
            hsv_to_rgb(AURORA_OFFSETS[1], 255, 40),
            "each period of the breath advances one waypoint"
        );
    }

    #[test]
    fn candle_stays_near_the_peak_and_flickers_without_panicking() {
        let params = effect_params(1_200);
        let floor = 200 - 200 / 6;
        let mut distinct = 0;
        let first = params.sample(Effect::Candle, 0).color;
        for t in (0..1_200).step_by(37) {
            let color = params.sample(Effect::Candle, t).color;
            let value = brightness_of(color);
            assert!(
                value >= floor && value <= 200,
                "t={t}: {value} out of range"
            );
            if color != first {
                distinct += 1;
            }
        }
        assert!(distinct > 0, "a candle flickers");
        let dark = EffectParams {
            max_brightness: 0,
            ..params
        };
        assert_eq!(dark.sample(Effect::Candle, 0).color, Rgb(0, 0, 0));
    }

    #[test]
    fn pulse_beats_twice_and_rests_between() {
        // period 1200 → beat 200, two beats over [0, 400) and [400, 800), then a gap.
        let params = effect_params(1_200);
        assert_eq!(params.sample(Effect::Pulse, 0).color, Rgb(0, 0, 0));
        assert_eq!(brightness_of(params.sample(Effect::Pulse, 200).color), 200);
        assert_eq!(brightness_of(params.sample(Effect::Pulse, 600).color), 200);
        assert_eq!(params.sample(Effect::Pulse, 1_000).color, Rgb(0, 0, 0));
    }

    #[test]
    fn heartbeat_beats_lub_dub_then_rests() {
        // period 1400 → lub 200, dub 140 starting at 300.
        let params = effect_params(1_400);
        assert_eq!(params.sample(Effect::Heartbeat, 0).color, Rgb(0, 0, 0));
        let lub = brightness_of(params.sample(Effect::Heartbeat, 100).color);
        let dub = brightness_of(params.sample(Effect::Heartbeat, 370).color);
        assert_eq!(lub, 200, "the first beat is full strength");
        assert!(dub > 100 && dub < lub, "the second beat is weaker");
        assert_eq!(
            params.sample(Effect::Heartbeat, 1_000).color,
            Rgb(0, 0, 0),
            "the rest is long"
        );
    }

    #[test]
    fn lightning_flashes_a_few_times_per_cycle() {
        for cycle in 0..3 {
            let params = effect_params(2_000);
            let mut bright = 0;
            let mut dark = 0;
            for t in 0..2_000 {
                let value =
                    brightness_of(params.sample(Effect::Lightning, cycle * 2_000 + t).color);
                assert!(value <= 200);
                if value > 100 {
                    bright += 1;
                } else {
                    dark += 1;
                }
                assert_eq!(
                    params.sample(Effect::Lightning, cycle * 2_000 + t),
                    params.sample(Effect::Lightning, cycle * 2_000 + t),
                    "the same instant is stable"
                );
            }
            assert!(bright > 0, "cycle {cycle} flashes");
            assert!(dark > bright, "cycle {cycle} is mostly dark");
        }
    }

    #[test]
    fn sparkle_twinkles_then_falls_dark() {
        let params = effect_params(2_000);
        let mut bright = 0;
        let mut dark = 0;
        for t in 0..2_000 {
            let value = brightness_of(params.sample(Effect::Sparkle, t).color);
            assert!(value <= 200);
            if value > 100 {
                bright += 1;
            } else {
                dark += 1;
            }
        }
        assert!(bright > 0, "some slots light");
        assert!(dark > bright, "most slots stay dark");
    }

    #[test]
    fn strobe_cuts_hard_between_on_and_off() {
        let params = effect_params(800);
        assert_eq!(brightness_of(params.sample(Effect::Strobe, 0).color), 200);
        assert_eq!(brightness_of(params.sample(Effect::Strobe, 99).color), 200);
        assert_eq!(params.sample(Effect::Strobe, 100).color, Rgb(0, 0, 0));
    }

    #[test]
    fn police_cuts_between_red_and_blue() {
        let params = effect_params(1_000);
        assert_eq!(
            params.sample(Effect::Police, 0).color,
            hsv_to_rgb(0, 255, 200)
        );
        assert_eq!(
            params.sample(Effect::Police, 499).color,
            hsv_to_rgb(0, 255, 200),
            "no interpolation before the switch"
        );
        assert_eq!(
            params.sample(Effect::Police, 500).color,
            hsv_to_rgb(170, 255, 200)
        );
        let warm = EffectParams {
            group: [16, 32, 3, 0, 0, 0, 0],
            group_len: 3,
            ..params
        };
        assert_eq!(
            warm.sample(Effect::Police, 0).color,
            hsv_to_rgb(0, 255, 200),
            "a walking group does not recolour the police"
        );
        let pair = EffectParams {
            group: [60, 120, 0, 0, 0, 0, 0],
            group_len: 2,
            ..params
        };
        assert_eq!(
            pair.sample(Effect::Police, 0).color,
            hsv_to_rgb(60, 255, 200),
            "a two-colour group is the police pair"
        );
    }

    #[test]
    fn sos_spells_three_short_three_long_three_short_then_silence() {
        // period 3600 → unit 100: S [0,500), gap [500,800), O [800,1900), gap [1900,2200),
        // S [2200,2500), then silence.
        let params = effect_params(3_600);
        assert_eq!(brightness_of(params.sample(Effect::Sos, 0).color), 200);
        assert_eq!(params.sample(Effect::Sos, 100).color, Rgb(0, 0, 0));
        assert_eq!(brightness_of(params.sample(Effect::Sos, 800).color), 200);
        assert_eq!(params.sample(Effect::Sos, 1_100).color, Rgb(0, 0, 0));
        assert_eq!(params.sample(Effect::Sos, 2_500).color, Rgb(0, 0, 0));
    }

    #[test]
    fn every_effect_is_dark_at_zero_and_stays_in_the_envelope() {
        for effect in [
            Effect::Steady,
            Effect::Breath,
            Effect::Cycle,
            Effect::Aurora,
            Effect::Candle,
            Effect::Pulse,
            Effect::Heartbeat,
            Effect::Lightning,
            Effect::Sparkle,
            Effect::Strobe,
            Effect::Police,
            Effect::Sos,
        ] {
            let params = effect_params(1_000);
            for t in [0, 123, 500, 999] {
                let value = brightness_of(params.sample(effect, t).color);
                assert!(value <= 200, "{effect:?} at {t}");
            }
            let dark = EffectParams {
                max_brightness: 0,
                ..params
            };
            assert_eq!(
                dark.sample(effect, 250).color,
                Rgb(0, 0, 0),
                "{effect:?} at zero"
            );
        }
    }
}
