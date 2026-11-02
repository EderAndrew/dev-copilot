//! Downmix to mono and resample to 16 kHz (the rate of every `AudioChunk`).
//!
//! Runs on the capture worker thread, never inside the real-time callback (Art. V.3).

use devcopilot_application::ports::SAMPLE_RATE_HZ;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler};
use thiserror::Error;

/// Input frames handed to the resampler per call.
const CHUNK_FRAMES: usize = 1024;

/// The device format cannot be converted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResampleError {
    /// Zero channels or a zero sample rate.
    #[error("unsupported input format")]
    UnsupportedFormat,
    /// The resampler rejected the conversion.
    #[error("resampler error: {0}")]
    Resampler(String),
}

/// Averages interleaved frames of `channels` samples into `out`.
pub fn downmix(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    #[allow(clippy::cast_precision_loss)] // channel counts are tiny
    let scale = 1.0 / channels as f32;
    out.extend(
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * scale),
    );
}

/// Streaming converter from interleaved device audio to 16 kHz mono `f32`.
pub struct ToMono16k {
    channels: usize,
    resampler: Option<Async<f32>>,
    pending: Vec<f32>,
    output: Vec<f32>,
}

impl ToMono16k {
    /// Converter for `channels` interleaved channels at `input_rate` Hz.
    ///
    /// # Errors
    ///
    /// [`ResampleError`] for degenerate formats.
    pub fn new(input_rate: u32, channels: u16) -> Result<Self, ResampleError> {
        if input_rate == 0 || channels == 0 {
            return Err(ResampleError::UnsupportedFormat);
        }
        let resampler = if input_rate == SAMPLE_RATE_HZ {
            None
        } else {
            let ratio = f64::from(SAMPLE_RATE_HZ) / f64::from(input_rate);
            Some(
                Async::<f32>::new_poly(
                    ratio,
                    1.0,
                    PolynomialDegree::Cubic,
                    CHUNK_FRAMES,
                    1,
                    FixedAsync::Input,
                )
                .map_err(|err| ResampleError::Resampler(err.to_string()))?,
            )
        };
        let output = vec![0.0; resampler.as_ref().map_or(0, Resampler::output_frames_max)];
        Ok(Self {
            channels: usize::from(channels),
            resampler,
            pending: Vec::with_capacity(CHUNK_FRAMES * 2),
            output,
        })
    }

    /// Converts `interleaved` and appends the 16 kHz mono samples produced so far to `out`.
    /// Up to one resampler chunk of input is kept until enough audio arrives.
    ///
    /// # Errors
    ///
    /// [`ResampleError::Resampler`] if the resampler fails.
    pub fn process(
        &mut self,
        interleaved: &[f32],
        out: &mut Vec<f32>,
    ) -> Result<(), ResampleError> {
        let Some(resampler) = self.resampler.as_mut() else {
            downmix(interleaved, self.channels, out);
            return Ok(());
        };
        downmix(interleaved, self.channels, &mut self.pending);
        let mut consumed = 0;
        while self.pending.len() - consumed >= resampler.input_frames_next() {
            let needed = resampler.input_frames_next();
            let input = &self.pending[consumed..consumed + needed];
            let adapter_in = InterleavedSlice::new(input, 1, needed)
                .map_err(|err| ResampleError::Resampler(err.to_string()))?;
            let frames_out = self.output.len();
            let mut adapter_out = InterleavedSlice::new_mut(&mut self.output, 1, frames_out)
                .map_err(|err| ResampleError::Resampler(err.to_string()))?;
            let (read, written) = resampler
                .process_into_buffer(&adapter_in, &mut adapter_out, None)
                .map_err(|err| ResampleError::Resampler(err.to_string()))?;
            out.extend_from_slice(&self.output[..written]);
            consumed += read;
        }
        self.pending.drain(..consumed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    #[allow(clippy::cast_precision_loss)]
    fn sine(rate: u32, freq: f32, secs: f32, channels: usize) -> Vec<f32> {
        let frames = (rate as f32 * secs) as usize;
        (0..frames)
            .flat_map(|i| {
                let v = (TAU * freq * i as f32 / rate as f32).sin() * 0.5;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    fn zero_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn downmix_averages_channels() {
        let mut out = Vec::new();
        downmix(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2, &mut out);
        assert_eq!(out, [0.5, 0.5, 0.0]);
    }

    #[test]
    fn mono_16k_passes_through() {
        let mut conv = ToMono16k::new(16_000, 1).unwrap();
        let input = sine(16_000, 440.0, 0.1, 1);
        let mut out = Vec::new();
        conv.process(&input, &mut out).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn stereo_48k_becomes_mono_16k_preserving_pitch() {
        let mut conv = ToMono16k::new(48_000, 2).unwrap();
        let input = sine(48_000, 440.0, 1.0, 2);
        let mut out = Vec::new();
        // Feed in uneven callback-sized blocks, as a device would.
        for block in input.chunks(2 * 480 + 6) {
            conv.process(block, &mut out).unwrap();
        }
        // ~1 s at 16 kHz, minus at most one buffered chunk and resampler delay.
        assert!(
            (15_000..=16_000).contains(&out.len()),
            "got {} samples",
            out.len()
        );
        // 440 Hz → ~880 zero crossings per second of output.
        #[allow(clippy::cast_precision_loss)]
        let per_sec = zero_crossings(&out) as f32 * 16_000.0 / out.len() as f32;
        assert!((860.0..=900.0).contains(&per_sec), "{per_sec} crossings/s");
        assert!(out.iter().all(|s| s.abs() <= 0.6));
    }

    #[test]
    fn rejects_degenerate_formats() {
        assert!(ToMono16k::new(0, 1).is_err());
        assert!(ToMono16k::new(48_000, 0).is_err());
    }
}
