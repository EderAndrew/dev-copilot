//! Energy-based voice activity detection and the sliding-window chunker used by local
//! transcription (research R4).
//!
//! The chunker turns a continuous 16 kHz stream into transcription windows:
//!
//! - silence before speech is not kept, apart from a short pre-roll so word onsets survive;
//! - while speech continues, a **partial** window (everything since the speech started) is cut
//!   every [`ChunkerConfig::partial_interval`];
//! - the window becomes **final** after [`ChunkerConfig::silence_to_finalize`] of silence, or
//!   when it reaches [`ChunkerConfig::max_window`];
//! - speech bursts shorter than [`ChunkerConfig::min_speech`] are discarded as noise;
//! - a discontinuity in the input (audio dropped upstream) finalizes the current window, so a
//!   window never spans a gap.

use devcopilot_application::ports::SAMPLE_RATE_HZ;
use devcopilot_domain::time::Duration;

/// Chunker tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkerConfig {
    /// Analysis frame length.
    pub frame: Duration,
    /// Root-mean-square level at or above which a frame counts as speech.
    pub speech_rms: f32,
    /// Silence that ends a window.
    pub silence_to_finalize: Duration,
    /// Time between partial windows while speech continues.
    pub partial_interval: Duration,
    /// Longest window.
    pub max_window: Duration,
    /// Shortest speech kept.
    pub min_speech: Duration,
    /// Audio kept before the first speech frame.
    pub pre_roll: Duration,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            frame: Duration::from_millis(30),
            // ≈ -38 dBFS: above typical room noise, below normal speech at a laptop mic.
            speech_rms: 0.012,
            silence_to_finalize: Duration::from_millis(700),
            partial_interval: Duration::from_secs(2),
            max_window: Duration::from_secs(25),
            min_speech: Duration::from_millis(250),
            pre_roll: Duration::from_millis(200),
        }
    }
}

/// Whether a window is provisional or final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutKind {
    /// Speech still going on; re-transcribed later.
    Partial,
    /// Window closed.
    Final,
}

/// A window to transcribe.
#[derive(Clone, PartialEq)]
pub struct Cut {
    /// Partial or final.
    pub kind: CutKind,
    /// Stream offset of the first sample.
    pub start: Duration,
    /// Stream offset after the last sample.
    pub end: Duration,
    /// 16 kHz mono samples.
    pub samples: Vec<f32>,
}

impl std::fmt::Debug for Cut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cut")
            .field("kind", &self.kind)
            .field("start", &self.start)
            .field("end", &self.end)
            .field("samples", &self.samples.len())
            .finish()
    }
}

/// Root-mean-square level of `samples`.
#[must_use]
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    #[allow(clippy::cast_precision_loss)] // frame lengths are tiny
    let len = samples.len() as f32;
    (sum / len).sqrt()
}

fn samples_for(duration: Duration) -> usize {
    usize::try_from(duration.as_millis() * u64::from(SAMPLE_RATE_HZ) / 1_000).unwrap_or(usize::MAX)
}

fn duration_of(samples: u64) -> Duration {
    Duration::from_millis(samples * 1_000 / u64::from(SAMPLE_RATE_HZ))
}

/// Tolerance before a jump in input offsets counts as a discontinuity.
const CONTINUITY_TOLERANCE: Duration = Duration::from_millis(50);

/// Sliding-window chunker for one source.
#[derive(Debug)]
pub struct Chunker {
    config: ChunkerConfig,
    frame_len: usize,
    /// Samples not yet forming a full frame.
    pending: Vec<f32>,
    /// Current window (pre-roll only while idle).
    window: Vec<f32>,
    /// Stream position, in samples, of `window[0]`.
    window_start: u64,
    /// Stream position, in samples, after the last sample received.
    position: u64,
    /// Offset of stream position 0.
    origin: Option<Duration>,
    active: bool,
    speech: Duration,
    trailing_silence: Duration,
    since_partial: Duration,
}

impl Chunker {
    /// Creates a chunker.
    #[must_use]
    pub fn new(config: ChunkerConfig) -> Self {
        Self {
            frame_len: samples_for(config.frame).max(1),
            config,
            pending: Vec::new(),
            window: Vec::new(),
            window_start: 0,
            position: 0,
            origin: None,
            active: false,
            speech: Duration::ZERO,
            trailing_silence: Duration::ZERO,
            since_partial: Duration::ZERO,
        }
    }

    fn offset(&self, position: u64) -> Duration {
        self.origin
            .unwrap_or(Duration::ZERO)
            .saturating_add(duration_of(position))
    }

    /// Feeds samples starting at stream offset `start`; returns the windows they complete.
    pub fn push(&mut self, start: Duration, samples: &[f32]) -> Vec<Cut> {
        let mut cuts = Vec::new();
        match self.origin {
            None => self.origin = Some(start),
            Some(_) => {
                let expected = self.offset(self.position);
                if start > expected.saturating_add(CONTINUITY_TOLERANCE) || start < expected {
                    // Audio dropped upstream (or reordered): close the window and restart the
                    // stream clock at `start`.
                    cuts.extend(self.flush());
                    self.pending.clear();
                    self.window.clear();
                    self.origin = Some(start);
                    self.position = 0;
                    self.window_start = 0;
                }
            }
        }
        self.pending.extend_from_slice(samples);
        let mut taken = 0;
        while self.pending.len() - taken >= self.frame_len {
            let frame: Vec<f32> = self.pending[taken..taken + self.frame_len].to_vec();
            taken += self.frame_len;
            if let Some(cut) = self.frame(&frame) {
                cuts.push(cut);
            }
        }
        self.pending.drain(..taken);
        cuts
    }

    fn frame(&mut self, frame: &[f32]) -> Option<Cut> {
        let speech = rms(frame) >= self.config.speech_rms;
        self.window.extend_from_slice(frame);
        self.position += frame.len() as u64;
        if !self.active {
            if speech {
                self.active = true;
                self.speech = self.config.frame;
                self.trailing_silence = Duration::ZERO;
                self.since_partial = Duration::ZERO;
            } else {
                let keep = samples_for(self.config.pre_roll);
                if self.window.len() > keep {
                    let excess = self.window.len() - keep;
                    self.window.drain(..excess);
                    self.window_start += excess as u64;
                }
            }
            return None;
        }
        if speech {
            self.speech = self.speech.saturating_add(self.config.frame);
            self.trailing_silence = Duration::ZERO;
        } else {
            self.trailing_silence = self.trailing_silence.saturating_add(self.config.frame);
        }
        self.since_partial = self.since_partial.saturating_add(self.config.frame);
        let length = duration_of(self.window.len() as u64);
        if self.trailing_silence >= self.config.silence_to_finalize
            || length >= self.config.max_window
        {
            return self.close();
        }
        if self.since_partial >= self.config.partial_interval {
            self.since_partial = Duration::ZERO;
            return Some(self.cut(CutKind::Partial));
        }
        None
    }

    fn cut(&self, kind: CutKind) -> Cut {
        Cut {
            kind,
            start: self.offset(self.window_start),
            end: self.offset(self.window_start + self.window.len() as u64),
            samples: self.window.clone(),
        }
    }

    /// Ends the active window: a final when it held enough speech, nothing otherwise.
    fn close(&mut self) -> Option<Cut> {
        let cut = (self.active && self.speech >= self.config.min_speech)
            .then(|| self.cut(CutKind::Final));
        self.window_start += self.window.len() as u64;
        self.window.clear();
        self.active = false;
        self.speech = Duration::ZERO;
        self.trailing_silence = Duration::ZERO;
        self.since_partial = Duration::ZERO;
        cut
    }

    /// Ends the stream: the pending window becomes final if it held enough speech.
    pub fn flush(&mut self) -> Option<Cut> {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            self.window.extend_from_slice(&rest);
            self.position += rest.len() as u64;
        }
        if self.active {
            self.close()
        } else {
            self.window_start += self.window.len() as u64;
            self.window.clear();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = 16_000;

    fn tone(secs_x10: usize) -> Vec<f32> {
        (0..RATE * secs_x10 / 10)
            .map(|i| {
                #[allow(clippy::cast_precision_loss)]
                let t = i as f32 / RATE as f32;
                0.2 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
            })
            .collect()
    }

    fn silence(secs_x10: usize) -> Vec<f32> {
        vec![0.0; RATE * secs_x10 / 10]
    }

    fn feed(chunker: &mut Chunker, start_ms: u64, samples: &[f32]) -> Vec<Cut> {
        // 100 ms chunks, as the microphone adapter delivers.
        samples
            .chunks(1_600)
            .enumerate()
            .flat_map(|(i, chunk)| {
                chunker.push(Duration::from_millis(start_ms + i as u64 * 100), chunk)
            })
            .collect()
    }

    fn kinds(cuts: &[Cut]) -> Vec<CutKind> {
        cuts.iter().map(|c| c.kind).collect()
    }

    #[test]
    fn rms_of_silence_and_tone() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&silence(1)), 0.0);
        let level = rms(&tone(1));
        assert!((level - 0.2 / 2_f32.sqrt()).abs() < 0.01);
    }

    #[test]
    fn silence_produces_nothing() {
        let mut chunker = Chunker::new(ChunkerConfig::default());
        assert!(feed(&mut chunker, 0, &silence(100)).is_empty());
        assert_eq!(chunker.flush(), None);
    }

    #[test]
    fn speech_then_pause_becomes_one_final_with_offsets() {
        let mut chunker = Chunker::new(ChunkerConfig::default());
        let mut audio = silence(10);
        audio.extend(tone(10));
        audio.extend(silence(10));
        let cuts = feed(&mut chunker, 0, &audio);
        assert_eq!(kinds(&cuts), [CutKind::Final]);
        let cut = &cuts[0];
        // Pre-roll of 200 ms before the speech at 1.0 s; ends after 700 ms of silence.
        assert!(cut.start >= Duration::from_millis(790) && cut.start <= Duration::from_millis(810));
        assert!(cut.end >= Duration::from_millis(2_690) && cut.end <= Duration::from_millis(2_750));
        assert_eq!(
            duration_of(cut.samples.len() as u64),
            cut.end.saturating_sub(cut.start)
        );
    }

    #[test]
    fn long_speech_emits_partials_every_two_seconds_then_a_final() {
        let mut chunker = Chunker::new(ChunkerConfig::default());
        let mut audio = tone(50);
        audio.extend(silence(10));
        let cuts = feed(&mut chunker, 0, &audio);
        assert_eq!(
            kinds(&cuts),
            [CutKind::Partial, CutKind::Partial, CutKind::Final]
        );
        // Partials grow from the same start.
        assert_eq!(cuts[0].start, cuts[1].start);
        assert!(cuts[1].samples.len() > cuts[0].samples.len());
        assert_eq!(cuts[2].start, cuts[0].start);
    }

    #[test]
    fn windows_are_capped_at_the_maximum_length() {
        let mut chunker = Chunker::new(ChunkerConfig::default());
        let cuts = feed(&mut chunker, 0, &tone(300));
        let finals: Vec<_> = cuts.iter().filter(|c| c.kind == CutKind::Final).collect();
        assert_eq!(finals.len(), 1);
        // Enforced at frame granularity.
        let length = finals[0].end.saturating_sub(finals[0].start);
        assert!(length >= Duration::from_secs(25) && length < Duration::from_millis(25_030));
        // The rest continues in a new window and is finalized by flush.
        let last = chunker.flush().unwrap();
        assert_eq!(last.start, finals[0].end);
    }

    #[test]
    fn short_noise_bursts_are_discarded() {
        let mut chunker = Chunker::new(ChunkerConfig::default());
        let mut audio = tone(1);
        audio.extend(silence(10));
        assert!(feed(&mut chunker, 0, &audio).is_empty());
    }

    #[test]
    fn a_discontinuity_closes_the_window() {
        let mut chunker = Chunker::new(ChunkerConfig::default());
        assert!(feed(&mut chunker, 0, &tone(10)).is_empty());
        // 15 s dropped upstream.
        let cuts = feed(&mut chunker, 16_000, &tone(5));
        assert_eq!(kinds(&cuts), [CutKind::Final]);
        assert!(cuts[0].end <= Duration::from_millis(1_000));
        let next = chunker.flush().unwrap();
        assert!(next.start >= Duration::from_secs(16));
    }

    #[test]
    fn offsets_follow_the_first_chunk() {
        let mut chunker = Chunker::new(ChunkerConfig::default());
        let mut audio = tone(5);
        audio.extend(silence(10));
        let cuts = feed(&mut chunker, 42_000, &audio);
        assert_eq!(cuts[0].start, Duration::from_secs(42));
    }

    #[test]
    fn flush_finalizes_pending_speech() {
        let mut chunker = Chunker::new(ChunkerConfig::default());
        assert!(feed(&mut chunker, 0, &tone(10)).is_empty());
        let cut = chunker.flush().unwrap();
        assert_eq!(cut.kind, CutKind::Final);
        assert_eq!(cut.end, Duration::from_secs(1));
        assert_eq!(chunker.flush(), None);
    }
}
