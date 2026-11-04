//! Whole-window arithmetic the board's frame accounting and the product's diagnostics share.

/// Repaint rate (frames per second) over one window, `0` for an empty or degenerate window.
pub fn windowed_rate(frames: u32, elapsed_ms: u64) -> u8 {
    if elapsed_ms == 0 {
        return 0;
    }
    u64::from(frames)
        .saturating_mul(1_000)
        .checked_div(elapsed_ms)
        .and_then(|rate| u8::try_from(rate).ok())
        .unwrap_or(u8::MAX)
}

/// A share of a window as a whole percent, or `0` for a window nothing was measured in.
pub fn windowed_percent(part_us: u64, window_us: u64) -> u32 {
    part_us
        .saturating_mul(100)
        .checked_div(window_us)
        .and_then(|pct| u32::try_from(pct).ok())
        .unwrap_or(100)
        .min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_reported_whole_and_never_over_a_hundred() {
        assert_eq!(windowed_percent(500_000, 2_000_000), 25);
        assert_eq!(windowed_percent(2_000_000, 2_000_000), 100);
        assert_eq!(windowed_percent(1, 1_000_000), 0);
        assert_eq!(windowed_percent(4_000_000, 2_000_000), 100);
        assert_eq!(windowed_percent(7_000, 0), 100);
    }

    #[test]
    fn rate_scales_frames_over_the_window() {
        assert_eq!(windowed_rate(13, 250), 52);
        assert_eq!(windowed_rate(0, 250), 0);
        assert_eq!(windowed_rate(5, 0), 0);
    }

    #[test]
    fn overheated_rate_saturates_at_max() {
        assert_eq!(windowed_rate(1_000, 1_000), u8::MAX);
        assert_eq!(windowed_rate(u32::MAX, 1), u8::MAX);
    }
}
