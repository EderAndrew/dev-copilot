//! Update use case with a scripted updater, fake session activity, in-memory settings and a
//! manual clock (002 T027, data-model §1.3, contracts/ipc-updates.md).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration as StdDuration;

use devcopilot_application::ApplicationError;
use devcopilot_application::ports::{AvailableUpdate, Clock, UpdateFailure};
use devcopilot_application::settings_service::SettingsService;
use devcopilot_application::testing::{
    FakeClock, FakeSessionActivity, InMemorySettingsRepository, ScriptedUpdater,
};
use devcopilot_application::update_service::{
    CHECK_INTERVAL, UpdateDeps, UpdateService, UpdateStatus,
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

struct Fixture {
    service: UpdateService,
    updater: Arc<ScriptedUpdater>,
    sessions: Arc<FakeSessionActivity>,
    settings: Arc<SettingsService>,
    clock: Arc<FakeClock>,
    status: watch::Receiver<UpdateStatus>,
}

fn fixture() -> Fixture {
    let updater = Arc::new(ScriptedUpdater::default());
    let sessions = Arc::new(FakeSessionActivity::default());
    let settings = Arc::new(SettingsService::new(Arc::new(
        InMemorySettingsRepository::default(),
    )));
    let clock = Arc::new(FakeClock::default());
    let service = UpdateService::new(UpdateDeps {
        updater: updater.clone(),
        sessions: sessions.clone(),
        settings: settings.clone(),
        clock: clock.clone(),
    });
    let status = service.subscribe();
    Fixture {
        service,
        updater,
        sessions,
        settings,
        clock,
        status,
    }
}

/// Notice dismissed, checks enabled.
fn ready() -> Fixture {
    let f = fixture();
    f.service.acknowledge_notice().unwrap();
    f
}

fn update(version: &str) -> AvailableUpdate {
    AvailableUpdate {
        version: version.into(),
        notes: "Correções".into(),
        published_at: None,
    }
}

impl Fixture {
    /// Waits (bounded) until the published status matches.
    async fn wait_for(&self, matches: impl Fn(&UpdateStatus) -> bool) -> UpdateStatus {
        let mut rx = self.status.clone();
        tokio::time::timeout(StdDuration::from_secs(2), rx.wait_for(|s| matches(s)))
            .await
            .expect("status not reached in time")
            .expect("publisher alive")
            .clone()
    }

    /// Checks and waits until `0.2.0` is available.
    async fn available(&self) {
        self.updater.set_check_result(Ok(Some(update("0.2.0"))));
        self.service.check().unwrap();
        self.wait_for(|s| matches!(s, UpdateStatus::Available(_)))
            .await;
    }
}

fn is_failed(status: &UpdateStatus) -> bool {
    matches!(status, UpdateStatus::Failed(_))
}

// --- gates: disabled and notice pending --------------------------------------------------------

#[tokio::test]
async fn disabled_checks_make_no_request() {
    let f = ready();
    f.service.set_check_enabled(false).unwrap();
    assert_eq!(f.service.status().unwrap(), UpdateStatus::Disabled);
    assert_eq!(
        f.service.check().unwrap_err(),
        ApplicationError::UpdatesDisabled
    );
    assert_eq!(
        f.service.install("0.2.0").unwrap_err(),
        ApplicationError::UpdatesDisabled
    );
    assert_eq!(f.updater.check_count(), 0);
}

#[tokio::test]
async fn pending_notice_makes_no_request() {
    let f = fixture();
    assert_eq!(f.service.status().unwrap(), UpdateStatus::NoticePending);
    assert_eq!(*f.status.borrow(), UpdateStatus::NoticePending);
    assert_eq!(
        f.service.check().unwrap_err(),
        ApplicationError::NoticePending
    );
    assert_eq!(f.updater.check_count(), 0);
}

#[tokio::test]
async fn acknowledging_the_notice_enables_checks() {
    let f = fixture();
    let settings = f.service.acknowledge_notice().unwrap();
    assert!(settings.update_notice_acknowledged);
    assert_eq!(
        f.service.status().unwrap(),
        UpdateStatus::Idle {
            last_checked_at: None
        }
    );
}

// --- check outcomes ----------------------------------------------------------------------------

#[tokio::test]
async fn check_finds_an_update_and_records_the_time() {
    let f = ready();
    f.updater.set_check_result(Ok(Some(update("0.2.0"))));
    assert_eq!(f.service.check().unwrap(), UpdateStatus::Checking);
    let status = f
        .wait_for(|s| matches!(s, UpdateStatus::Available(_)))
        .await;
    assert_eq!(status, UpdateStatus::Available(update("0.2.0")));
    assert_eq!(
        f.settings.update_last_checked_at().unwrap(),
        Some(f.clock.now())
    );
    assert_eq!(f.updater.check_count(), 1);
}

#[tokio::test]
async fn check_reports_up_to_date() {
    let f = ready();
    f.updater.set_check_result(Ok(None));
    f.service.check().unwrap();
    let status = f
        .wait_for(|s| matches!(s, UpdateStatus::UpToDate { .. }))
        .await;
    assert_eq!(
        status,
        UpdateStatus::UpToDate {
            last_checked_at: f.clock.now()
        }
    );
}

#[tokio::test]
async fn check_failure_keeps_the_previous_check_time() {
    let f = ready();
    f.updater.set_check_result(Err(UpdateFailure::Network));
    f.service.check().unwrap();
    let status = f.wait_for(is_failed).await;
    assert_eq!(status, UpdateStatus::Failed(UpdateFailure::Network));
    assert_eq!(f.settings.update_last_checked_at().unwrap(), None);
    assert_eq!(f.updater.install_count(), 0);
}

// --- install -----------------------------------------------------------------------------------

#[tokio::test]
async fn install_downloads_installs_and_restarts() {
    let f = ready();
    f.available().await;
    let status = f.service.install("0.2.0").unwrap();
    assert!(
        matches!(status, UpdateStatus::Downloading { ref version, .. } if version == "0.2.0"),
        "{status:?}"
    );
    f.wait_for(|s| matches!(s, UpdateStatus::Installing { .. }))
        .await;
    tokio::time::timeout(StdDuration::from_secs(2), async {
        while f.updater.restart_count() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.updater.install_count(), 1);
}

#[tokio::test]
async fn install_without_an_available_update_is_invalid() {
    let f = ready();
    assert!(matches!(
        f.service.install("0.2.0").unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
}

#[tokio::test]
async fn install_of_another_version_is_refused() {
    let f = ready();
    f.available().await;
    assert_eq!(
        f.service.install("0.3.0").unwrap_err(),
        ApplicationError::VersionMismatch
    );
    assert_eq!(f.updater.download_count(), 0);
}

#[tokio::test]
async fn install_is_blocked_while_a_session_is_active() {
    let f = ready();
    f.available().await;
    f.sessions.set_active(true);
    assert_eq!(
        f.service.install("0.2.0").unwrap(),
        UpdateStatus::BlockedBySession {
            version: "0.2.0".into()
        }
    );
    assert_eq!(f.updater.download_count(), 0);
    // Once the session ends the update is offered again and can be confirmed.
    f.sessions.set_active(false);
    assert_eq!(
        f.service.status().unwrap(),
        UpdateStatus::Available(update("0.2.0"))
    );
    f.service.install("0.2.0").unwrap();
    f.wait_for(|s| matches!(s, UpdateStatus::Installing { .. }))
        .await;
}

#[tokio::test]
async fn session_started_during_download_blocks_the_install() {
    let f = ready();
    f.available().await;
    f.updater.hold_downloads();
    f.service.install("0.2.0").unwrap();
    f.sessions.set_active(true);
    f.updater.release_downloads();
    let status = f
        .wait_for(|s| matches!(s, UpdateStatus::BlockedBySession { .. }))
        .await;
    assert_eq!(
        status,
        UpdateStatus::BlockedBySession {
            version: "0.2.0".into()
        }
    );
    assert_eq!(f.updater.install_count(), 0);
    assert_eq!(f.updater.restart_count(), 0);
}

#[tokio::test]
async fn download_progress_is_published() {
    let f = ready();
    f.available().await;
    f.updater.hold_downloads();
    f.service.install("0.2.0").unwrap();
    let status = f
        .wait_for(|s| {
            matches!(
                s,
                UpdateStatus::Downloading {
                    received_bytes: 10,
                    ..
                }
            )
        })
        .await;
    assert_eq!(
        status,
        UpdateStatus::Downloading {
            version: "0.2.0".into(),
            received_bytes: 10,
            total_bytes: Some(100)
        }
    );
    f.updater.release_downloads();
}

// --- single flight and cancellation ------------------------------------------------------------

#[tokio::test]
async fn concurrent_operations_are_refused() {
    let f = ready();
    f.updater.hold_checks();
    f.service.check().unwrap();
    assert!(matches!(
        f.service.check().unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    assert!(matches!(
        f.service.install("0.2.0").unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    f.updater.release_checks();

    f.wait_for(|s| matches!(s, UpdateStatus::UpToDate { .. }))
        .await;
    f.available().await;
    f.updater.hold_downloads();
    f.service.install("0.2.0").unwrap();
    assert!(matches!(
        f.service.check().unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    assert!(matches!(
        f.service.install("0.2.0").unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    f.updater.release_downloads();
}

#[tokio::test]
async fn cancel_requires_an_operation() {
    let f = ready();
    assert!(matches!(
        f.service.cancel().unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
}

#[tokio::test]
async fn cancelled_check_result_is_dropped() {
    let f = ready();
    f.updater.hold_checks();
    f.updater.set_check_result(Ok(Some(update("0.2.0"))));
    f.service.check().unwrap();
    assert_eq!(
        f.service.cancel().unwrap(),
        UpdateStatus::Idle {
            last_checked_at: None
        }
    );
    f.updater.release_checks();
    tokio::time::sleep(StdDuration::from_millis(20)).await;
    assert_eq!(
        f.service.status().unwrap(),
        UpdateStatus::Idle {
            last_checked_at: None
        }
    );
}

#[tokio::test]
async fn cancelled_download_returns_to_available_without_installing() {
    let f = ready();
    f.available().await;
    f.updater.hold_downloads();
    f.service.install("0.2.0").unwrap();
    assert_eq!(
        f.service.cancel().unwrap(),
        UpdateStatus::Available(update("0.2.0"))
    );
    tokio::time::timeout(StdDuration::from_secs(2), async {
        while !f.updater.download_was_cancelled() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.updater.install_count(), 0);
}

#[tokio::test]
async fn disabling_cancels_a_check_in_progress() {
    let f = ready();
    f.updater.hold_checks();
    f.updater.set_check_result(Ok(Some(update("0.2.0"))));
    f.service.check().unwrap();
    let settings = f.service.set_check_enabled(false).unwrap();
    assert!(!settings.update_check_enabled);
    assert_eq!(f.service.status().unwrap(), UpdateStatus::Disabled);
    f.updater.release_checks();
    tokio::time::sleep(StdDuration::from_millis(20)).await;
    // Re-enabling does not resurrect the abandoned result.
    f.service.set_check_enabled(true).unwrap();
    assert_eq!(
        f.service.status().unwrap(),
        UpdateStatus::Idle {
            last_checked_at: None
        }
    );
}

#[tokio::test]
async fn disabling_cancels_a_download_in_progress() {
    let f = ready();
    f.available().await;
    f.updater.hold_downloads();
    f.service.install("0.2.0").unwrap();
    f.service.set_check_enabled(false).unwrap();
    assert_eq!(*f.status.borrow(), UpdateStatus::Disabled);
    tokio::time::timeout(StdDuration::from_secs(2), async {
        while !f.updater.download_was_cancelled() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.updater.install_count(), 0);
}

// --- failures never install --------------------------------------------------------------------

#[tokio::test]
async fn every_download_failure_leaves_the_installed_version_unchanged() {
    for reason in [
        UpdateFailure::Network,
        UpdateFailure::SignatureInvalid,
        UpdateFailure::ManifestInvalid,
        UpdateFailure::DownloadInterrupted,
    ] {
        let f = ready();
        f.available().await;
        f.updater.set_download_result(Err(reason));
        f.service.install("0.2.0").unwrap();
        let status = f.wait_for(is_failed).await;
        assert_eq!(status, UpdateStatus::Failed(reason));
        assert_eq!(f.updater.install_count(), 0, "{reason:?}");
        assert_eq!(f.updater.restart_count(), 0, "{reason:?}");
        // A failed update must be checked again before another install.
        assert!(matches!(
            f.service.install("0.2.0").unwrap_err(),
            ApplicationError::InvalidState(_)
        ));
    }
}

#[tokio::test]
async fn install_failure_does_not_restart() {
    let f = ready();
    f.available().await;
    f.updater
        .set_install_result(Err(UpdateFailure::InstallFailed));
    f.service.install("0.2.0").unwrap();
    let status = f.wait_for(is_failed).await;
    assert_eq!(status, UpdateStatus::Failed(UpdateFailure::InstallFailed));
    assert_eq!(f.updater.restart_count(), 0);
}

#[tokio::test]
async fn notes_from_the_manifest_are_capped() {
    let f = ready();
    let mut long = update("0.2.0");
    long.notes = "x".repeat(10_000);
    f.updater.set_check_result(Ok(Some(long)));
    f.service.check().unwrap();
    let status = f
        .wait_for(|s| matches!(s, UpdateStatus::Available(_)))
        .await;
    let UpdateStatus::Available(found) = status else {
        unreachable!()
    };
    assert_eq!(found.notes.chars().count(), 4_000);
}

// --- scheduler ---------------------------------------------------------------------------------

async fn settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn scheduler_checks_at_startup_and_every_24_hours() {
    let f = ready();
    let shutdown = CancellationToken::new();
    let service = f.service.clone();
    let task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { service.run_scheduler(shutdown).await }
    });
    settle().await;
    assert_eq!(f.updater.check_count(), 1, "startup check");
    tokio::time::advance(CHECK_INTERVAL - StdDuration::from_secs(1)).await;
    settle().await;
    assert_eq!(f.updater.check_count(), 1);
    tokio::time::advance(StdDuration::from_secs(2)).await;
    settle().await;
    assert_eq!(f.updater.check_count(), 2, "24 h later");
    shutdown.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn scheduler_waits_for_the_notice_and_respects_the_toggle() {
    let f = fixture();
    let shutdown = CancellationToken::new();
    let service = f.service.clone();
    let task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { service.run_scheduler(shutdown).await }
    });
    settle().await;
    tokio::time::advance(CHECK_INTERVAL * 2).await;
    settle().await;
    assert_eq!(f.updater.check_count(), 0, "notice still pending");

    f.service.acknowledge_notice().unwrap();
    settle().await;
    assert_eq!(f.updater.check_count(), 1, "first check after the notice");

    f.service.set_check_enabled(false).unwrap();
    tokio::time::advance(CHECK_INTERVAL * 3).await;
    settle().await;
    assert_eq!(f.updater.check_count(), 1, "no check while disabled");

    f.service.set_check_enabled(true).unwrap();
    settle().await;
    assert_eq!(f.updater.check_count(), 2, "re-enabling checks right away");
    shutdown.cancel();
    task.await.unwrap();
}
