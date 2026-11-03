//! The committed entity changes handed to live evaluation and not yet
//! evaluated, by the instant each committed at.
//!
//! A change is held from the moment it is handed off until it is dropped,
//! evaluated or discarded alike, so the earliest instant held bounds which
//! selections a change still to be evaluated can have committed under. The
//! subscription cache keeps a selection an update superseded only while that
//! bound lies before the selection's end.

use andromeda_context_commit::{context_change_event::ContextChangeEvent, store_instant::StoreInstant};
use parking_lot::Mutex;
use std::{collections::BTreeMap, sync::Arc};

/// The commit instants of the changes held, each with how many are held.
#[derive(Debug, Default)]
pub struct PendingCommits {
    /// Count of held changes per commit instant; an instant with none is
    /// absent.
    held: Mutex<BTreeMap<StoreInstant, usize>>,
}

impl PendingCommits {
    /// Hold `change` until the returned [`PendingChange`] is dropped.
    #[must_use]
    pub fn hold(self: &Arc<PendingCommits>, change: ContextChangeEvent) -> PendingChange {
        let committed_at = change.committed_at;
        *self.held.lock().entry(committed_at).or_default() += 1;
        PendingChange {
            change,
            hold: CommitHold {
                commits: Arc::clone(self),
                committed_at,
            },
        }
    }

    /// The earliest commit instant held, `None` when no change is.
    #[must_use]
    pub fn floor(&self) -> Option<StoreInstant> {
        self.held.lock().first_key_value().map(|(committed_at, _)| *committed_at)
    }

    /// How many changes are held.
    #[must_use]
    pub fn count(&self) -> usize {
        self.held.lock().values().sum()
    }

    /// Stop holding one change committed at `committed_at`.
    fn release(&self, committed_at: StoreInstant) {
        let mut held = self.held.lock();
        if let Some(count) = held.get_mut(&committed_at) {
            *count -= 1;
            if *count == 0 {
                held.remove(&committed_at);
            }
        }
    }
}

/// One held change's place in [`PendingCommits`], given up when dropped.
#[derive(Debug)]
pub struct CommitHold {
    /// The changes it is held among.
    commits: Arc<PendingCommits>,
    /// The instant the change committed at.
    committed_at: StoreInstant,
}

/// Releasing the hold is the cleanup of dropping it, which no derive provides.
impl Drop for CommitHold {
    fn drop(&mut self) {
        self.commits.release(self.committed_at);
    }
}

/// A committed entity change handed to live evaluation, held until dropped.
#[derive(Debug)]
pub struct PendingChange {
    /// The change.
    pub change: ContextChangeEvent,
    /// Its hold, released when the change is dropped.
    pub hold: CommitHold,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::pending_change::PendingCommits;
    use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent, store_instant::StoreInstant};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
    };
    use chrono::{DateTime, Utc};
    use std::sync::Arc;

    fn at(unix_seconds: i64) -> StoreInstant {
        StoreInstant::new(DateTime::from_timestamp(unix_seconds, 0).expect("a fixture timestamp in range"))
    }

    fn committed(unix_seconds: i64) -> ContextChangeEvent {
        ContextChangeEvent {
            target: ContextTarget::Live,
            event: ChangeEvent::builder()
                .entity_id("urn:ngsi-ld:Vehicle:held".parse::<Identifier>().expect("a valid identifier"))
                .change(EntityChange::new(EntityLifecycle::Modified, Vec::new()))
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .build(),
            committed_at: at(unix_seconds),
        }
    }

    #[test]
    fn nothing_held_has_no_floor() {
        pretty_assertions::assert_eq!(Arc::new(PendingCommits::default()).floor(), None);
    }

    #[test]
    fn the_floor_is_the_earliest_commit_held_whatever_order_the_changes_came_in() {
        let commits = Arc::new(PendingCommits::default());
        let later = commits.hold(committed(2_000));
        let earlier = commits.hold(committed(1_000));
        pretty_assertions::assert_eq!(commits.floor(), Some(at(1_000)));
        drop(earlier);
        pretty_assertions::assert_eq!(commits.floor(), Some(at(2_000)));
        drop(later);
        pretty_assertions::assert_eq!(commits.floor(), None);
    }

    /// Two changes of one write share its commit instant; the instant stays
    /// held until both are dropped.
    #[test]
    fn an_instant_two_changes_share_is_held_until_both_are_dropped() {
        let commits = Arc::new(PendingCommits::default());
        let first = commits.hold(committed(1_000));
        let second = commits.hold(committed(1_000));
        drop(first);
        pretty_assertions::assert_eq!(commits.floor(), Some(at(1_000)));
        drop(second);
        pretty_assertions::assert_eq!(commits.floor(), None);
    }
}
