//! The selections Subscriptions held before an update replaced them, kept while
//! a change committed under one may still be evaluated.
//!
//! Clause 5.8.6 owes a notification for a change of a watched Attribute, and
//! an update (clause 5.8.2.4) modifies the Subscription without withdrawing
//! what a change committed before it already owes. A change is evaluated after
//! its commit, so one committed while a selection stood can reach evaluation
//! after an update replaced it. Such a change is matched against the selection
//! that stood at its commit: the version in force over
//! `[its selected instant, the next version's selected instant)`.
//!
//! A version is kept only while a change committed before its interval ended
//! may still be evaluated, which the evaluation reports as the earliest commit
//! instant it still holds.

use crate::compiled::CompiledSubscription;
use andromeda_context_commit::store_instant::StoreInstant;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::sync::Arc;

/// One selection a Subscription held before an update replaced it.
#[derive(Debug)]
struct SupersededSelection {
    /// The Subscription as it stood while the selection was in force; its
    /// `notifying_since.selected` is when the interval began.
    selection: Arc<CompiledSubscription>,
    /// When the next version's selection took over: the interval's exclusive
    /// end.
    until: StoreInstant,
}

impl SupersededSelection {
    /// Whether the selection stood when a change committed at `committed_at`.
    fn stood_at(&self, committed_at: StoreInstant) -> bool {
        self.selection.notifying_since.selected <= committed_at && committed_at < self.until
    }

    /// Whether a change still to be evaluated, none committed before `floor`,
    /// can have committed while the selection stood; with no `floor`, none is
    /// left to evaluate.
    fn awaited_by(&self, floor: Option<StoreInstant>) -> bool {
        floor.is_some_and(|floor| floor < self.until)
    }
}

/// The superseded selections of one target's Subscriptions.
///
/// The versions of one Subscription have disjoint intervals, each ending where
/// the next begins, and the last ends where the cached version's selection
/// begins, so a commit instant falls in at most one of them and never in one
/// of them and the cached version both.
#[derive(Debug, Default)]
pub(crate) struct SupersededSelections {
    /// Every version kept, in the order it was superseded.
    versions: Vec<SupersededSelection>,
}

impl SupersededSelections {
    /// Keep `previous` when `next` replaces it with a selection that began
    /// later.
    ///
    /// A periodic `previous` is not kept: no change was owed to it (Table
    /// 5.2.12-1 `timeInterval`).
    pub(crate) fn supersede(&mut self, previous: Arc<CompiledSubscription>, next: &CompiledSubscription) {
        let until = next.notifying_since.selected;
        if previous.time_interval_secs.is_none() && previous.notifying_since.selected < until {
            self.versions.push(SupersededSelection { selection: previous, until });
        }
    }

    /// Drop every version of the Subscription `id`, which is no longer
    /// notified of changes.
    pub(crate) fn forget(&mut self, id: &Identifier) {
        self.versions.retain(|version| version.selection.id != *id);
    }

    /// The versions that stood when a change committed at `committed_at`.
    pub(crate) fn standing_at(&self, committed_at: StoreInstant) -> impl Iterator<Item = &Arc<CompiledSubscription>> {
        self.versions
            .iter()
            .filter(move |version| version.stood_at(committed_at))
            .map(|version| &version.selection)
    }

    /// Whether a version is no longer awaited by any change committed at or
    /// after `floor`.
    pub(crate) fn releases(&self, floor: Option<StoreInstant>) -> bool {
        self.versions.iter().any(|version| !version.awaited_by(floor))
    }

    /// Drop every version no change committed at or after `floor` can have
    /// committed under.
    pub(crate) fn release(&mut self, floor: Option<StoreInstant>) {
        self.versions.retain(|version| version.awaited_by(floor));
    }

    /// Number of versions kept.
    pub(crate) const fn len(&self) -> usize {
        self.versions.len()
    }
}
