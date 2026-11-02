//! Microphone capture over `cpal` (research R1) implementing the application `AudioSource`.
//!
//! Each capture runs on its own thread, which owns the platform stream:
//!
//! - the real-time callback only converts samples to `f32` and pushes them into a bounded
//!   `rtrb` ring (~2 s); it never allocates, locks or does I/O (Art. V.3);
//! - the thread drains the ring every few milliseconds, downmixes and resamples to 16 kHz mono
//!   ([`ToMono16k`]) and hands 100 ms chunks to the pipeline sink without blocking;
//! - the stream is bound to the concrete device resolved at start. A disconnect or a change of
//!   the OS default device never moves capture to another device (FR-011): it ends the
//!   capture and is reported as [`SourceFault::Lost`].
//!
//! Permission: `cpal` has no permission API. The state is the last one observed when opening a
//! stream; [`AudioSource::request_permission`] opens a short probe stream, which is what
//! triggers the OS prompt on macOS (`NSMicrophoneUsageDescription`). The probe's audio is
//! discarded inside the adapter.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration as StdDuration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use devcopilot_application::ports::{
    AudioChunk, AudioError, AudioSource, BoundedAudioSink, BoxFuture, CaptureHandle, SinkError,
    SourceFault, SourceFaultNotifier,
};
use devcopilot_domain::audio_source::{
    AudioSourceKind, Availability, PermissionState, UnavailableReason,
};
use devcopilot_domain::time::Duration;
use rtrb::{Consumer, Producer, RingBuffer};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::resample::ToMono16k;

/// Samples per chunk handed to the pipeline (100 ms at 16 kHz).
const CHUNK_SAMPLES: usize = 1_600;
/// Seconds of device audio the ring holds.
const RING_SECONDS: usize = 2;
/// How often the worker drains the ring.
const POLL_INTERVAL: StdDuration = StdDuration::from_millis(10);
/// Deadline for the backend to open the stream.
const OPEN_TIMEOUT: StdDuration = StdDuration::from_secs(5);
/// How long the permission probe keeps its stream open.
const PROBE_DURATION: StdDuration = StdDuration::from_millis(300);

fn permission_code(state: PermissionState) -> u8 {
    match state {
        PermissionState::NotRequested => 0,
        PermissionState::Granted => 1,
        PermissionState::Denied => 2,
        PermissionState::Revoked => 3,
    }
}

fn permission_from(code: u8) -> PermissionState {
    match code {
        1 => PermissionState::Granted,
        2 => PermissionState::Denied,
        3 => PermissionState::Revoked,
        _ => PermissionState::NotRequested,
    }
}

/// Maps a stream-open failure; never includes device names or content.
fn open_error(err: &cpal::Error) -> AudioError {
    match err.kind() {
        ErrorKind::PermissionDenied => AudioError::PermissionNotGranted(PermissionState::Denied),
        ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => {
            AudioError::Unavailable(Availability::Unavailable(UnavailableReason::NoDevice))
        }
        kind => AudioError::Backend(kind.to_string()),
    }
}

/// Maps a running-stream error to a fault; `None` for recoverable glitches.
fn stream_fault(kind: ErrorKind) -> Option<SourceFault> {
    match kind {
        ErrorKind::PermissionDenied => Some(SourceFault::PermissionRevoked),
        // `DeviceChanged` would mean the backend followed a new default device; capture must
        // not continue on a device the user did not select (FR-011).
        ErrorKind::DeviceNotAvailable
        | ErrorKind::DeviceChanged
        | ErrorKind::StreamInvalidated
        | ErrorKind::HostUnavailable
        | ErrorKind::BackendError
        | ErrorKind::Other => Some(SourceFault::Lost),
        _ => None,
    }
}

/// Opened stream plus its sample ring.
struct OpenStream {
    stream: Stream,
    consumer: Consumer<f32>,
    converter: ToMono16k,
    channels: usize,
}

/// Default input device, resolved to the concrete device so the stream never follows later
/// default-device changes.
fn bound_device(host: &cpal::Host) -> Result<cpal::Device, AudioError> {
    let no_device =
        || AudioError::Unavailable(Availability::Unavailable(UnavailableReason::NoDevice));
    let default = host.default_input_device().ok_or_else(no_device)?;
    match default.id() {
        Ok(id) => host.device_by_id(&id).ok_or_else(no_device),
        Err(_) => Ok(default),
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut producer: Producer<f32>,
    dropped: Arc<AtomicU64>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device.build_input_stream::<T, _, _>(
        *config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            // Real-time path: no allocation, lock or I/O.
            for &sample in data {
                if producer.push(sample.to_sample::<f32>()).is_err() {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        },
        on_error,
        Some(OPEN_TIMEOUT),
    )
}

fn open_stream(
    on_error: impl FnMut(cpal::Error) + Send + 'static,
    dropped: Arc<AtomicU64>,
) -> Result<OpenStream, AudioError> {
    let host = cpal::default_host();
    let device = bound_device(&host)?;
    let supported = device.default_input_config().map_err(|e| open_error(&e))?;
    let config = supported.config();
    let channels = usize::from(config.channels);
    let rate = config.sample_rate;
    let capacity = usize::try_from(rate)
        .unwrap_or(48_000)
        .saturating_mul(channels)
        .saturating_mul(RING_SECONDS);
    let (producer, consumer) = RingBuffer::<f32>::new(capacity.max(CHUNK_SAMPLES));
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, producer, dropped, on_error),
        SampleFormat::I16 => build::<i16>(&device, &config, producer, dropped, on_error),
        SampleFormat::U16 => build::<u16>(&device, &config, producer, dropped, on_error),
        SampleFormat::I32 => build::<i32>(&device, &config, producer, dropped, on_error),
        other => {
            return Err(AudioError::Backend(format!(
                "unsupported sample format {other}"
            )));
        }
    }
    .map_err(|e| open_error(&e))?;
    stream.play().map_err(|e| open_error(&e))?;
    let converter = ToMono16k::new(rate, config.channels)
        .map_err(|err| AudioError::Backend(err.to_string()))?;
    Ok(OpenStream {
        stream,
        consumer,
        converter,
        channels,
    })
}

/// The system's default microphone.
#[derive(Debug, Default)]
pub struct CpalMicrophone {
    permission: Arc<AtomicU8>,
}

impl CpalMicrophone {
    /// Creates the adapter; nothing is opened until a capture starts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn set_permission(&self, state: PermissionState) {
        self.permission
            .store(permission_code(state), Ordering::SeqCst);
    }
}

impl AudioSource for CpalMicrophone {
    fn kind(&self) -> AudioSourceKind {
        AudioSourceKind::Microphone
    }

    fn availability(&self) -> Availability {
        if cpal::default_host().default_input_device().is_some() {
            Availability::Available
        } else {
            Availability::Unavailable(UnavailableReason::NoDevice)
        }
    }

    fn permission(&self) -> PermissionState {
        permission_from(self.permission.load(Ordering::SeqCst))
    }

    fn request_permission(&self) -> BoxFuture<'_, PermissionState> {
        let (tx, rx) = oneshot::channel();
        let spawned = std::thread::Builder::new()
            .name("devcopilot-mic-probe".into())
            .spawn(move || {
                let result = open_stream(|_| {}, Arc::new(AtomicU64::new(0))).map(|open| {
                    std::thread::sleep(PROBE_DURATION);
                    drop(open.stream);
                });
                let _ = tx.send(result);
            });
        Box::pin(async move {
            let state = match spawned {
                Err(_) => self.permission(),
                Ok(_) => match rx.await {
                    Ok(Ok(())) => PermissionState::Granted,
                    Ok(Err(AudioError::PermissionNotGranted(_))) => PermissionState::Denied,
                    // No device or backend failure: the permission itself is still unknown.
                    Ok(Err(_)) | Err(_) => self.permission(),
                },
            };
            self.set_permission(state);
            state
        })
    }

    fn start(
        &self,
        sink: BoundedAudioSink,
        cancel: CancellationToken,
        faults: SourceFaultNotifier,
    ) -> Result<Box<dyn CaptureHandle>, AudioError> {
        let permission = self.permission();
        if !permission.is_granted() {
            return Err(AudioError::PermissionNotGranted(permission));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = Worker {
            sink,
            cancel,
            faults,
            stop: Arc::clone(&stop),
            permission: Arc::clone(&self.permission),
        };
        let thread = std::thread::Builder::new()
            .name("devcopilot-mic".into())
            .spawn(move || worker.run(&ready_tx))
            .map_err(|err| {
                AudioError::Backend(format!("cannot spawn capture thread: {}", err.kind()))
            })?;
        let handle = CpalCaptureHandle {
            stop,
            thread: Some(thread),
        };
        match ready_rx.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(())) => Ok(Box::new(handle)),
            Ok(Err(err)) => {
                if matches!(err, AudioError::PermissionNotGranted(_)) {
                    self.set_permission(PermissionState::Denied);
                }
                Err(err)
            }
            Err(_) => Err(AudioError::Backend("stream did not open in time".into())),
        }
    }
}

struct Worker {
    sink: BoundedAudioSink,
    cancel: CancellationToken,
    faults: SourceFaultNotifier,
    stop: Arc<AtomicBool>,
    permission: Arc<AtomicU8>,
}

impl Worker {
    fn run(self, ready: &mpsc::SyncSender<Result<(), AudioError>>) {
        let faulted = Arc::new(AtomicBool::new(false));
        let on_error = {
            let faults = self.faults.clone();
            let faulted = Arc::clone(&faulted);
            let permission = Arc::clone(&self.permission);
            move |err: cpal::Error| {
                let Some(fault) = stream_fault(err.kind()) else {
                    return;
                };
                if fault == SourceFault::PermissionRevoked {
                    permission.store(permission_code(PermissionState::Revoked), Ordering::SeqCst);
                }
                faulted.store(true, Ordering::SeqCst);
                faults.notify(fault);
            }
        };
        let dropped = Arc::new(AtomicU64::new(0));
        let mut open = match open_stream(on_error, Arc::clone(&dropped)) {
            Ok(open) => open,
            Err(err) => {
                let _ = ready.send(Err(err));
                return;
            }
        };
        if ready.send(Ok(())).is_err() {
            return; // `start` gave up waiting; dropping the stream closes it.
        }
        tracing::info!(channels = open.channels, "microphone capture open");

        let mut device_samples = Vec::with_capacity(CHUNK_SAMPLES * 8);
        let mut mono = Vec::with_capacity(CHUNK_SAMPLES * 4);
        let mut emitted: u64 = 0;
        let mut overloaded: u64 = 0;
        while !self.stop.load(Ordering::SeqCst)
            && !self.cancel.is_cancelled()
            && !faulted.load(Ordering::SeqCst)
        {
            std::thread::sleep(POLL_INTERVAL);
            device_samples.clear();
            while let Ok(sample) = open.consumer.pop() {
                device_samples.push(sample);
            }
            if open.converter.process(&device_samples, &mut mono).is_err() {
                self.faults.notify(SourceFault::Lost);
                break;
            }
            while mono.len() >= CHUNK_SAMPLES {
                let samples: Vec<f32> = mono.drain(..CHUNK_SAMPLES).collect();
                let chunk = AudioChunk {
                    source: AudioSourceKind::Microphone,
                    start: Duration::from_millis(
                        emitted * 1_000 / u64::from(devcopilot_application::ports::SAMPLE_RATE_HZ),
                    ),
                    samples,
                };
                emitted += CHUNK_SAMPLES as u64;
                match self.sink.try_send(chunk) {
                    Ok(()) => {}
                    // Overload policy and gaps are applied downstream (FR-019, US2).
                    Err(SinkError::Full) => overloaded += 1,
                    Err(SinkError::Closed) => return,
                }
            }
        }
        // Dropping the stream closes the OS capture before the handle's `stop` returns.
        drop(open);
        tracing::info!(
            dropped_samples = dropped.load(Ordering::Relaxed),
            overloaded_chunks = overloaded,
            "microphone capture closed"
        );
    }
}

struct CpalCaptureHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CpalCaptureHandle {
    fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("microphone capture thread panicked");
        }
    }
}

impl CaptureHandle for CpalCaptureHandle {
    fn stop(mut self: Box<Self>) {
        self.close();
    }
}

impl Drop for CpalCaptureHandle {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_codes_round_trip() {
        for state in [
            PermissionState::NotRequested,
            PermissionState::Granted,
            PermissionState::Denied,
            PermissionState::Revoked,
        ] {
            assert_eq!(permission_from(permission_code(state)), state);
        }
    }

    #[test]
    fn default_device_change_is_a_fault_not_a_switch() {
        assert_eq!(
            stream_fault(ErrorKind::DeviceChanged),
            Some(SourceFault::Lost)
        );
        assert_eq!(
            stream_fault(ErrorKind::DeviceNotAvailable),
            Some(SourceFault::Lost)
        );
        assert_eq!(
            stream_fault(ErrorKind::PermissionDenied),
            Some(SourceFault::PermissionRevoked)
        );
        assert_eq!(stream_fault(ErrorKind::Xrun), None);
        assert_eq!(stream_fault(ErrorKind::RealtimeDenied), None);
    }

    #[test]
    fn open_errors_map_without_device_details() {
        assert_eq!(
            open_error(&cpal::Error::new(ErrorKind::PermissionDenied)),
            AudioError::PermissionNotGranted(PermissionState::Denied)
        );
        assert_eq!(
            open_error(&cpal::Error::with_message(
                ErrorKind::DeviceNotAvailable,
                "USB Headset (Jane's)"
            )),
            AudioError::Unavailable(Availability::Unavailable(UnavailableReason::NoDevice))
        );
        let backend = open_error(&cpal::Error::with_message(
            ErrorKind::DeviceBusy,
            "Jane's mic",
        ));
        assert!(!format!("{backend}").contains("Jane"));
    }

    #[test]
    fn start_without_permission_captures_nothing() {
        let mic = CpalMicrophone::new();
        let (sink, _rx) = BoundedAudioSink::channel(1);
        let (faults, _faults_rx) = SourceFaultNotifier::channel();
        let result = mic.start(sink, CancellationToken::new(), faults);
        assert!(matches!(
            result.err(),
            Some(AudioError::PermissionNotGranted(
                PermissionState::NotRequested
            ))
        ));
    }
}
