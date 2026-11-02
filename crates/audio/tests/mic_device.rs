//! Captures from the real default microphone (T053). Ignored by default: needs a device and,
//! on macOS, microphone permission for the terminal. Run with
//! `cargo test -p devcopilot-audio --test mic_device -- --ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use devcopilot_application::ports::{AudioSource, BoundedAudioSink, SourceFaultNotifier};
use devcopilot_audio::cpal_mic::CpalMicrophone;
use devcopilot_domain::audio_source::PermissionState;
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real microphone and OS permission"]
async fn captures_16k_mono_chunks_and_stops_promptly() {
    let mic = CpalMicrophone::new();
    assert!(mic.availability().is_available(), "no input device");
    assert_eq!(mic.request_permission().await, PermissionState::Granted);

    let (sink, mut rx) = BoundedAudioSink::channel(64);
    let (faults, _faults_rx) = SourceFaultNotifier::channel();
    let handle = mic.start(sink, CancellationToken::new(), faults).unwrap();

    let first = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("no audio within 3 s")
        .expect("sink closed");
    assert_eq!(first.samples.len(), 1_600, "100 ms at 16 kHz");
    assert!(
        first
            .samples
            .iter()
            .all(|s| s.is_finite() && s.abs() <= 1.0)
    );

    let stop_started = Instant::now();
    handle.stop();
    assert!(
        stop_started.elapsed() < Duration::from_secs(1),
        "stop took {:?} (SC-002)",
        stop_started.elapsed()
    );
    // Drain what was queued before stop; then the stream must be closed.
    while rx.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rx.try_recv().is_err(), "audio arrived after stop");
}
