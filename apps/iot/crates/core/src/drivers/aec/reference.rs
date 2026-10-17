//! What the speaker played, held far enough back for the canceller to be handed
//! the sample that was *leaving* the codec rather than the one on its way there.
//!
//! **The write pointer is the wrong place to tap.** A streaming transmit ring is
//! refilled well ahead of the wire — 24 descriptors of 960 bytes is 5 760 frames,
//! **120 ms of audio still to come** — so tapping where samples are written hands
//! the canceller a reference that far ahead of the echo it has to cancel, against
//! a search covering [`MAX_DELAY`](super::MAX_DELAY) samples, 4 ms at 16 kHz.
//! Thirty times outside the window: the search answers "nothing found" and the
//! filter converges on whatever noise the grid favoured.
//!
//! So the ring is written on the same cadence as the transmit ring's own pushes,
//! which is what makes it a faithful copy, and read through a **play cursor** the
//! driver advances from the DMA's own free-space reading. What comes out is the
//! frame the wire was carrying — the only frame whose acoustic path the
//! microphone is currently hearing.
//!
//! The log has to span the transmit ring's whole runway, the receive ring's
//! latency and the search window, see [`required_frames`]. An undersized log does
//! not read wrong, it reads *nothing*, which is why the drivers assert it.
//!
//! Samples arrive at the codec's rate and the canceller wants 16 kHz, so the log
//! keeps one frame in [`DECIMATION`]. Its phase is a parameter because the two
//! transfers number their frames independently — see
//! [`Decimator`](crate::drivers::audio::decimator::Decimator).
use crate::drivers::audio::decimator::{DECIMATION, Decimator};

/// The canceller's search window in frames, at the codec's rate: how far ahead
/// of the play cursor a caller may legitimately ask for.
pub const SEARCH_FRAMES: usize = super::MAX_DELAY * DECIMATION as usize;

/// Frames of log the worst case needs: everything the transmit ring can still be
/// holding, plus everything the receive ring can still be holding undrained,
/// plus the search window. A caller that is behind by more than this has lost
/// the alignment, and the only correct answer is to say so.
pub const fn required_frames(transmit_runway: usize, receive_latency: usize) -> usize {
    transmit_runway + receive_latency + SEARCH_FRAMES
}

/// How many kept samples to reach back across a capture ring that was `lag_frames`
/// deep when it was read.
///
/// **Rounds up, and the direction matters.** The lag is measured in frames at the
/// codec's rate and the run is decimated, so the lag has to be converted to whole
/// kept samples. Rounding *down* reaches back less far than the ring actually was
/// deep, which leaves the pair a fraction of a sample out of place; rounding up
/// reaches back at least as far, which leaves it out by a fraction the other way
/// and still within the window. A whole kept sample out is a third of a canceller
/// sample — recoverable, and nothing like the whole-run error of getting the lag
/// wrong by not applying it at all.
pub const fn lag_in_samples(lag_frames: usize, decimation: usize) -> usize {
    lag_frames.div_ceil(decimation)
}

/// Where the wire is, in frames, from what the CPU has pushed and how much of
/// the ring the DMA has freed.
///
/// **Occupancy is not position.** The free space is the run of audio the DMA
/// has finished sending *and that has not yet been overwritten*, so it is
/// `ring − (pushed − played)` — the queue depth, not how far along the stream
/// the wire is. Reading it as the position puts the wire at the end of the ring
/// at boot, where nothing has been sent at all, and every pair drawn from that
/// log is garbage that looks like a real signal.
///
/// `pushed_frames` counts every frame the CPU has handed the ring since it was
/// armed, **including the prefill**: a ring handed to the DMA already holds a
/// whole ring of frames that have not been sent, and leaving those out of the
/// count is what makes the first reading wrong rather than merely early.
pub const fn wire_position(
    pushed_frames: usize,
    ring_frames: usize,
    free_bytes: usize,
    frame_bytes: usize,
) -> usize {
    let queued = ring_frames.saturating_sub(free_bytes / frame_bytes);
    pushed_frames.saturating_sub(queued)
}

/// A copy of the transmitted stream, read back at the play cursor.
///
/// `N` is the number of decimated samples held, which is [`required_frames`]
/// divided by [`DECIMATION`] — carried as a const generic so a caller cannot
/// size it by a number the compiler cannot check against its own geometry.
#[derive(Debug, Clone)]
pub struct ReferenceLog<const N: usize> {
    samples: [i16; N],
    decimator: Decimator,
    /// Counts **kept samples**, not frames: one frame in three is written, so
    /// indexing the ring by frame number would address three slots per sample
    /// and leave two thirds of the ring holding whatever was there before.
    written: usize,
    /// Kept samples the DMA has finished sending, and so the log position the
    /// wire is at. Monotonic, and never ahead of [`Self::written`].
    played: usize,
}

impl<const N: usize> ReferenceLog<N> {
    /// An empty log. `phase` picks which frame of each group is kept.
    pub const fn new(phase: u8) -> Self {
        Self {
            samples: [0; N],
            decimator: Decimator::with_phase(phase),
            written: 0,
            played: 0,
        }
    }

    /// Takes one transmitted frame in, at the codec's rate. Only the frames this
    /// log's phase keeps occupy a slot.
    pub fn push(&mut self, sample: i16) {
        if let Some(kept) = self.decimator.push(sample) {
            self.samples[self.written % N] = kept;
            self.written += 1;
        }
    }

    /// The DMA has finished `frames` more frames than it had, and the log's read
    /// cursor follows. The frames are converted to kept samples first, so a
    /// driver reports what its DMA says in the same unit the caller pushes and
    /// reads. Saturating so a driver that reads its free space after a restart
    /// cannot walk the cursor off the end of what it has.
    pub fn played(&mut self, frames: usize) {
        let kept = frames / DECIMATION as usize;
        self.played = self.played.saturating_add(kept).min(self.written);
    }

    /// The sample for the frame the wire is `back` frames behind the play
    /// cursor, and whether the log still holds it.
    ///
    /// `back` counts frames at the codec's rate, the unit a driver's DMA reports
    /// in and the unit a decimated microphone stream is indexed by, and it is
    /// converted here to the kept samples the ring is actually addressed in.
    /// Rounding down answers with the sample covering the frame asked for rather
    /// than the one after it, which is the half-open convention the canceller's
    /// own ring uses: a frame not yet sent is not part of what the microphone
    /// heard.
    ///
    /// `None` in three cases, and each is an answer the caller has to act on:
    /// nothing has been sent yet, so there is no frame on the wire to name; the
    /// ask reaches past the start of the stream; or the log has been lapped, so
    /// the frame it would hand back belongs to a lap ago and cancelling against
    /// it would be worse than not cancelling. See [`Self::lagged_samples`].
    pub fn at_play_cursor(&self, back: usize) -> Option<i16> {
        let kept_back = back / DECIMATION as usize;
        // `played` counts samples *finished*, so the one on the wire is the one
        // before it — an off-by-one here reads the frame still queued rather
        // than the frame sounding, which is the whole point of this type.
        if self.played == 0 || self.has_lapped() || kept_back >= self.played {
            return None;
        }
        Some(self.samples[(self.played - 1 - kept_back) % N])
    }

    /// Kept samples held but not yet played, which is the log's own depth. Zero
    /// before the first frame has gone out, and never above [`Self::capacity`].
    pub const fn held(&self) -> usize {
        self.written - self.played
    }

    /// Kept samples the log holds, which is the ceiling [`Self::held`] is bounded
    /// by.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// How far past the end of the log the play cursor is, in **kept samples** —
    /// the unit the ring is addressed in, and the one [`Self::at_play_cursor`]
    /// compares against. Zero while the alignment holds, so a caller reporting
    /// this instead of only acting on a `None` can say which of the two it was.
    pub const fn lagged_samples(&self) -> usize {
        self.held().saturating_sub(N)
    }

    /// Whether the log has been lapped, which is the condition under which the
    /// caller must stop pairing rather than pair against a stale frame.
    pub const fn has_lapped(&self) -> bool {
        self.lagged_samples() > 0
    }

    /// The phase the log keeps, so a caller pairing it against a microphone
    /// stream can match it rather than guess.
    pub const fn phase(&self) -> u8 {
        self.decimator.phase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// Frames of distinct values, so a sample read back can be traced to the
    /// frame that produced it.
    fn frames(count: usize) -> Vec<i16> {
        (0..count).map(|n| (n as i16).wrapping_mul(97)).collect()
    }

    /// A log with room to spare, having been fed `written` frames with its play
    /// cursor `played` frames behind the wire. Both arguments are in frames, the
    /// unit a driver reports in.
    fn fed(written: usize, played: usize, phase: u8) -> ReferenceLog<256> {
        let mut log = ReferenceLog::<256>::new(phase);
        for &frame in &frames(written) {
            log.push(frame);
        }
        log.played(played);
        log
    }

    #[test]
    fn the_runway_a_transmit_ring_holds_is_the_reason_this_exists() {
        // The numbers from `audio_out.rs`: 24 descriptors of 960 bytes, four
        // bytes a frame, at 48 kHz. If these ever stop describing the ring, the
        // sizing argument below stops describing it too.
        let transmit_ring_bytes = 24 * 960;
        let frame_bytes = 4;
        let transmit_runway = transmit_ring_bytes / frame_bytes;
        assert_eq!(transmit_runway, 5_760);
        let wire_ms = transmit_runway * 1_000 / 48_000;
        assert_eq!(wire_ms, 120);
        // And the search window that 120 ms has to fit inside, in the same units.
        assert_eq!(SEARCH_FRAMES, 64 * 3);
        assert!(
            transmit_runway > SEARCH_FRAMES * 2,
            "the write pointer is {wire_ms} ms ahead of the wire and the search covers \
             {} frames: tapping the write pointer puts the reference outside the window",
            SEARCH_FRAMES
        );
    }

    #[test]
    fn only_one_frame_in_three_occupies_a_slot() {
        // The indexing this type lives or dies by. 300 frames at one in three is
        // 100 kept samples, and if the ring were addressed by frame number the
        // cursor would be reading slots that were never written.
        let log = fed(300, 0, 0);
        assert_eq!(log.written, 100, "300 frames at one in three");
        assert_eq!(log.capacity(), 256);
        assert_eq!(log.held(), 100);
    }

    #[test]
    fn a_frame_read_at_the_play_cursor_is_the_frame_that_went_out() {
        // Values, not positions: `frames(n) = n * 97` and the log keeps every
        // third frame, so kept index `k` holds frame `3k` and a wrong index
        // cannot look right.
        let sample_of_frame = |frame: usize| (frame as i16).wrapping_mul(97);

        let log = fed(300, 0, 0);
        assert_eq!(
            log.at_play_cursor(0),
            None,
            "nothing sent yet means no frame on the wire to name"
        );

        let log = fed(300, 300, 0);
        assert_eq!(
            log.at_play_cursor(0),
            Some(sample_of_frame(297)),
            "300 frames written and played is 100 kept samples, the last being frame 297"
        );
    }

    #[test]
    fn the_cursor_follows_the_dma_rather_than_the_writer() {
        let sample_of_frame = |frame: usize| (frame as i16).wrapping_mul(97);
        let mut log = fed(300, 0, 0);
        assert_eq!(log.at_play_cursor(0), None, "the wire has not started");
        log.played(100);
        assert_eq!(log.played, 33, "100 frames is 33 kept samples");
        assert_eq!(
            log.at_play_cursor(0),
            Some(sample_of_frame(96)),
            "the wire has reached frame 96, not the frame 297 the writer is on"
        );
        assert_ne!(
            log.at_play_cursor(0),
            Some(sample_of_frame(297)),
            "the write pointer would have handed back a frame 200 ms from the wire"
        );
    }

    #[test]
    fn reading_back_reaches_further_into_the_past() {
        let sample_of_frame = |frame: usize| (frame as i16).wrapping_mul(97);
        let log = fed(300, 300, 0);
        // Reading back is a coarser walk than the cursor: the log is decimated,
        // so a frame count that is not a whole multiple of the decimation lands
        // on the sample covering that frame. Three frames back is one kept
        // sample back — the finest step the log can express — and four rounds
        // down onto the same one.
        assert_eq!(
            log.at_play_cursor(3),
            Some(sample_of_frame(294)),
            "three frames back is one kept sample back"
        );
        assert_eq!(
            log.at_play_cursor(4),
            log.at_play_cursor(3),
            "and four rounds down onto the same sample"
        );
        assert_eq!(
            log.at_play_cursor(6),
            Some(sample_of_frame(291)),
            "six frames back is two kept samples back"
        );
    }

    #[test]
    fn a_cursor_far_enough_back_runs_out_of_log() {
        let log = fed(300, 300, 0);
        assert!(log.at_play_cursor(30).is_some(), "inside the window");
        assert!(
            log.at_play_cursor(3_000).is_none(),
            "3 000 frames back is past everything the log holds"
        );
    }

    #[test]
    fn a_lapped_log_says_so_rather_than_handing_back_a_stale_frame() {
        // The failure this exists for: a log too short for the runway hands back
        // a frame from a lap ago, which is a real sample and the wrong one.
        let mut log = ReferenceLog::<32>::new(0);
        for &frame in &frames(300) {
            log.push(frame);
        }
        // The writer runs ahead and the DMA has sent nothing, which is the state
        // a starved transmit ring is in — the log has lapped long before the
        // caller notices anything odd.
        assert_eq!(log.written, 100, "300 frames is 100 kept samples");
        assert_eq!(log.played, 0);
        assert!(
            log.has_lapped(),
            "nothing played means all 100 kept samples are held"
        );
        assert_eq!(log.held(), 100, "the writer has run 100 samples ahead");
        assert_eq!(log.lagged_samples(), 68, "100 held into a 32-sample ring");
        assert!(
            log.at_play_cursor(0).is_none(),
            "and it must refuse rather than answer from the lap"
        );
    }

    #[test]
    fn a_log_with_room_does_not_report_a_lag() {
        let log = fed(300, 300, 0);
        assert_eq!(log.lagged_samples(), 0);
        assert!(!log.has_lapped());
        assert_eq!(log.held(), 0, "everything written has gone out");
    }

    #[test]
    fn a_capture_ring_s_lag_rounds_up_into_whole_samples() {
        // The lag is measured in frames and the run is decimated, so it has to be
        // converted. Rounding down would reach back less far than the ring was
        // deep — a fraction of a sample out, in the direction that silently
        // decorrelates the pair from its own echo.
        let decimation = 3;
        assert_eq!(
            lag_in_samples(0, decimation),
            0,
            "an empty ring needs no reach"
        );
        assert_eq!(lag_in_samples(1, decimation), 1);
        assert_eq!(
            lag_in_samples(3, decimation),
            1,
            "a whole group is one sample"
        );
        assert_eq!(
            lag_in_samples(4, decimation),
            2,
            "four frames is one group and a bit, so two samples' worth of reach"
        );
        assert_eq!(
            lag_in_samples(2_880, decimation),
            960,
            "this board's full ring"
        );

        // And never less than the ring's own depth in samples.
        for frames in (0..600).step_by(7) {
            assert!(
                lag_in_samples(frames, decimation) * decimation >= frames,
                "{frames} frames rounded down to {} samples",
                lag_in_samples(frames, decimation)
            );
        }
    }

    #[test]
    fn the_wire_is_where_the_dma_left_it_not_where_the_ring_is_full() {
        // The bug this arithmetic exists to prevent. Occupancy and position look
        // alike and are not: at boot the ring is *full* of silence the DMA has
        // never sent, so reading free space as the position says the wire is a
        // whole ring ahead of where it is, and every pair drawn from that log is
        // garbage that looks like a signal.
        let ring_frames = 5_760;
        let frame_bytes = 4;

        // A ring handed to the DMA prefilled: a whole ring pushed, nothing freed.
        assert_eq!(
            wire_position(ring_frames, ring_frames, 0, frame_bytes),
            0,
            "nothing has been sent yet, so the wire is at the start"
        );

        // 240 frames later the DMA has freed exactly one 5 ms chunk.
        let sent = 240;
        let free = sent * frame_bytes;
        assert_eq!(
            wire_position(ring_frames, ring_frames, free, frame_bytes),
            sent,
            "the wire has moved by what the DMA freed, and by nothing else"
        );

        // A push does not move the wire. Read at the same instant — pushed
        // *after* the push, and the free space the push left behind. Mixing the
        // two instants is how a correct function gets a wrong answer, which is
        // why both numbers move together here.
        let pushed_again = ring_frames + sent;
        assert_eq!(
            wire_position(pushed_again, ring_frames, 0, frame_bytes),
            sent,
            "a push queues audio, it does not send it"
        );

        // And the next chunk the DMA frees does move it.
        assert_eq!(
            wire_position(pushed_again, ring_frames, free, frame_bytes),
            2 * sent,
            "the wire follows the DMA's free space, not the writer's"
        );
    }

    #[test]
    fn the_wire_is_never_reported_ahead_of_what_was_pushed() {
        // A DMA that reports more free space than was ever queued — a stalled
        // engine, or a ring read after a restart — must not put the wire past the
        // end of what exists. This is the guarantee the driver's own saturating
        // delta then builds on.
        let ring_frames = 5_760;
        for pushed in [0_usize, 1, 100, 5_760] {
            let played = wire_position(pushed, ring_frames, ring_frames * 4, 4);
            assert!(
                played <= pushed,
                "{pushed} frames pushed reported the wire at {played}"
            );
        }
    }

    #[test]
    fn the_play_cursor_never_runs_past_what_was_written() {
        // A driver reading its free space after a restart can report more frames
        // than exist; the cursor saturating is what keeps a wrapped ring from
        // indexing backwards into the whole log.
        let mut log = fed(30, 0, 0);
        log.played(1_000_000);
        assert_eq!(log.played, 10, "30 frames is 10 kept samples");
        assert_eq!(log.held(), 0);
    }

    #[test]
    fn a_phase_picks_which_frame_of_each_group_the_log_keeps() {
        // The pairing requirement: the microphone's decimator has to keep the
        // same frame of each group this one does, and this says which that is.
        assert_eq!(fed(300, 0, 0).phase(), 0);
        assert_eq!(fed(300, 0, 1).phase(), 1);
        assert_eq!(fed(300, 0, 2).phase(), 2);
        // And the phase really does move which frame is kept, which is what makes
        // the microphone's phase something that has to be matched rather than
        // assumed to agree.
        assert_eq!(
            fed(300, 300, 1).at_play_cursor(0),
            Some((298_i16).wrapping_mul(97)),
            "phase 1 keeps frames 1, 4, … 298, so the last of 300 is frame 298"
        );
    }

    #[test]
    fn the_cursor_reads_are_stable_while_nothing_is_played() {
        // The caller only advances the cursor when the DMA says it has, so a
        // poll that finds no new audio must not be able to shift the reference.
        let log = fed(300, 120, 0);
        let first = log.at_play_cursor(0);
        for _ in 0..50 {
            assert_eq!(
                log.at_play_cursor(0),
                first,
                "an idle poll moved the reference"
            );
        }
    }

    #[test]
    fn the_sizing_helper_covers_both_rings_and_the_search_window() {
        // What the drivers assert against, with the geometry they assert it
        // from: a 120 ms transmit runway and a 60 ms receive ring.
        let transmit_runway = 24 * 960 / 4;
        let receive_latency = 3 * 3_840 / 4;
        assert_eq!(transmit_runway, 5_760, "120 ms of transmit runway");
        assert_eq!(receive_latency, 2_880, "60 ms of receive ring");
        let needed = required_frames(transmit_runway, receive_latency);
        assert_eq!(needed, 5_760 + 2_880 + SEARCH_FRAMES);
        assert_eq!(needed, 8_832, "in frames at the codec's rate");
        // Sizing the ring is the frame count divided by the decimation, and the
        // ceiling has to round *up*: a ring one sample short silently laps.
        let ring = needed.div_ceil(DECIMATION as usize);
        assert_eq!(ring, 2_944);
        assert!(
            ring * DECIMATION as usize >= needed,
            "{ring} samples cover {needed} frames only if it rounds up"
        );
    }
}
