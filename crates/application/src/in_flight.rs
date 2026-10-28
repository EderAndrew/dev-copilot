//! Per-session registry of background work (analysis runs, questions), so deleting a session
//! can cancel what still runs for it and wait until it has stopped writing (FR-043, spec edge
//! case "Exclusão durante análise ou pergunta em andamento").

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use devcopilot_domain::ids::SessionId;
use tokio_util::sync::CancellationToken;

struct Entry {
    key: u64,
    cancel: CancellationToken,
    done: CancellationToken,
}

#[derive(Default)]
struct Registry {
    entries: Mutex<HashMap<SessionId, Vec<Entry>>>,
    next_key: AtomicU64,
}

impl Registry {
    fn lock(&self) -> MutexGuard<'_, HashMap<SessionId, Vec<Entry>>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Background work registered per session.
#[derive(Clone, Default)]
pub(crate) struct InFlight {
    registry: Arc<Registry>,
}

/// Registration of one piece of work. Dropping it (when the work ends, however it ends)
/// unregisters it and wakes anyone waiting in [`InFlight::cancel`].
pub(crate) struct Ticket {
    registry: Arc<Registry>,
    session: SessionId,
    key: u64,
    cancel: CancellationToken,
    done: CancellationToken,
}

impl InFlight {
    /// Registers work for `session`.
    pub(crate) fn register(&self, session: SessionId) -> Ticket {
        let mut entries = self.registry.lock();
        self.insert(&mut entries, session)
    }

    /// Registers work for `session` unless some is already registered for it.
    pub(crate) fn register_exclusive(&self, session: SessionId) -> Option<Ticket> {
        let mut entries = self.registry.lock();
        if entries.get(&session).is_some_and(|list| !list.is_empty()) {
            return None;
        }
        Some(self.insert(&mut entries, session))
    }

    fn insert(&self, entries: &mut HashMap<SessionId, Vec<Entry>>, session: SessionId) -> Ticket {
        let key = self.registry.next_key.fetch_add(1, Ordering::Relaxed);
        let cancel = CancellationToken::new();
        let done = CancellationToken::new();
        entries.entry(session).or_default().push(Entry {
            key,
            cancel: cancel.clone(),
            done: done.clone(),
        });
        Ticket {
            registry: Arc::clone(&self.registry),
            session,
            key,
            cancel,
            done,
        }
    }

    /// Cancels every piece of work registered for `session` and waits up to `deadline` for all
    /// of it to end. `false` when some work outlived the deadline.
    pub(crate) async fn cancel(&self, session: SessionId, deadline: std::time::Duration) -> bool {
        let waiting: Vec<CancellationToken> = self
            .registry
            .lock()
            .get(&session)
            .map(|list| {
                list.iter()
                    .map(|entry| {
                        entry.cancel.cancel();
                        entry.done.clone()
                    })
                    .collect()
            })
            .unwrap_or_default();
        tokio::time::timeout(deadline, async {
            for done in waiting {
                done.cancelled().await;
            }
        })
        .await
        .is_ok()
    }
}

impl Ticket {
    /// Fires when the work must stop.
    pub(crate) fn token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut entries = self.registry.lock();
        if let Some(list) = entries.get_mut(&self.session) {
            list.retain(|entry| entry.key != self.key);
            if list.is_empty() {
                entries.remove(&self.session);
            }
        }
        drop(entries);
        self.done.cancel();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn exclusive_registration_waits_for_the_previous_ticket() {
        let registry = InFlight::default();
        let session = SessionId::new();
        let ticket = registry.register_exclusive(session).unwrap();
        assert!(registry.register_exclusive(session).is_none());
        assert!(registry.register_exclusive(SessionId::new()).is_some());
        drop(ticket);
        assert!(registry.register_exclusive(session).is_some());
    }

    #[tokio::test]
    async fn cancel_fires_tokens_and_waits_for_tickets_to_drop() {
        let registry = InFlight::default();
        let session = SessionId::new();
        let first = registry.register(session);
        let second = registry.register(session);
        let other = registry.register(SessionId::new());
        let worker = tokio::spawn(async move {
            first.token().cancelled().await;
            second.token().cancelled().await;
            tokio::time::sleep(Duration::from_millis(10)).await;
            drop((first, second));
        });
        assert!(registry.cancel(session, Duration::from_secs(2)).await);
        assert!(!other.token().is_cancelled());
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn cancel_reports_work_that_outlives_the_deadline() {
        let registry = InFlight::default();
        let session = SessionId::new();
        let _stuck = registry.register(session);
        assert!(!registry.cancel(session, Duration::from_millis(10)).await);
    }

    #[tokio::test]
    async fn cancel_without_work_returns_at_once() {
        assert!(
            InFlight::default()
                .cancel(SessionId::new(), Duration::from_millis(1))
                .await
        );
    }
}
