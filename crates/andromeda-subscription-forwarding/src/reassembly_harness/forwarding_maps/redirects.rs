//! The mappings of a re-assembly test as the redirect ledger. Adopting a
//! receiver other than the one adopted last owes it the companion and every
//! copy the sources hold or a settled claim records, and the test makes a
//! retry due at will rather than by waiting.

use crate::{
    error::SubscriptionForwardingError,
    reassembly_harness::forwarding_maps::{ClaimLease, ForwardingMaps},
    receiver::ReceiverEndpoint,
    redirect_ledger::{OwedRedirect, RedirectLedger, RedirectTarget},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use async_trait::async_trait;
use std::time::{Duration, Instant};

impl ForwardingMaps {
    /// Whether `target` is owed the current receiver.
    pub(crate) fn owes(&self, target: &RedirectTarget) -> bool {
        self.owed
            .lock()
            .expect("the owed redirects are never poisoned")
            .iter()
            .any(|(owed, _)| owed == target)
    }

    /// Whether `target` is owed the current receiver with its update not due
    /// yet.
    pub(crate) fn owes_later(&self, target: &RedirectTarget) -> bool {
        let now = Instant::now();
        self.owed
            .lock()
            .expect("the owed redirects are never poisoned")
            .iter()
            .any(|(owed, due)| owed == target && *due > now)
    }

    /// Make every owed update due now, as the time it waits for would.
    pub(crate) fn end_redirect_retries(&self) {
        let now = Instant::now();
        self.owed
            .lock()
            .expect("the owed redirects are never poisoned")
            .iter_mut()
            .for_each(|(_, due)| *due = now);
    }

    /// Owe `target` the current receiver from now, unless it is owed already.
    pub(super) fn owe(&self, target: RedirectTarget) {
        let mut owed = self.owed.lock().expect("the owed redirects are never poisoned");
        if !owed.iter().any(|(held, _)| *held == target) {
            owed.push((target, Instant::now()));
        }
    }

    /// The copy recorded at `registration`, as a kept claim or a note records it.
    pub(super) fn copy_at(&self, registration: &Identifier) -> Option<ForwardedCopy> {
        let kept = self
            .records
            .lock()
            .expect("the claim records are never poisoned")
            .iter()
            .find(|record| record.registration == *registration)
            .map(|record| ForwardedCopy {
                registration: record.registration.clone(),
                remote_subscription: record.remote.clone(),
            });
        kept.or_else(|| {
            self.claimed
                .lock()
                .expect("the claims are never poisoned")
                .iter()
                .rev()
                .find(|claim| claim.registration == *registration)
                .cloned()
        })
    }

    /// Forget every owed update whose target `forgotten` picks.
    pub(super) fn forget_redirects(&self, forgotten: impl Fn(&RedirectTarget) -> bool) {
        self.owed
            .lock()
            .expect("the owed redirects are never poisoned")
            .retain(|(owed, _)| !forgotten(owed));
    }
}

#[async_trait]
impl RedirectLedger for ForwardingMaps {
    async fn adopt_receiver(&self, receiver: &ReceiverEndpoint) -> Result<(), SubscriptionForwardingError> {
        let mut adopted = self.adopted.lock().expect("the adopted receiver is never poisoned");
        if adopted.as_ref() == Some(receiver) {
            return Ok(());
        }
        *adopted = Some(receiver.clone());
        drop(adopted);
        self.owe(RedirectTarget::Companion);
        let held = self.copies.lock().expect("the copies are never poisoned").clone();
        let settled: Vec<ForwardedCopy> = self
            .records
            .lock()
            .expect("the claim records are never poisoned")
            .iter()
            .filter(|record| record.lease == ClaimLease::Settled)
            .map(|record| ForwardedCopy {
                registration: record.registration.clone(),
                remote_subscription: record.remote.clone(),
            })
            .collect();
        held.into_iter().chain(settled).for_each(|copy| self.owe(RedirectTarget::Copy(copy)));
        Ok(())
    }

    async fn overdue_redirects(&self, limit: u32) -> Result<Vec<OwedRedirect>, SubscriptionForwardingError> {
        let now = Instant::now();
        let mut due: Vec<(RedirectTarget, Instant)> = self
            .owed
            .lock()
            .expect("the owed redirects are never poisoned")
            .iter()
            .filter(|(_, due)| *due <= now)
            .cloned()
            .collect();
        due.sort_by_key(|(target, due)| (matches!(target, RedirectTarget::Copy(_)), *due));
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let companions = due.iter().filter(|(target, _)| *target == RedirectTarget::Companion).take(limit);
        let copies = due.iter().filter(|(target, _)| matches!(target, RedirectTarget::Copy(_))).take(limit);
        Ok(companions
            .chain(copies)
            .map(|(target, _)| OwedRedirect {
                local: self.local.clone(),
                target: target.clone(),
            })
            .collect())
    }

    async fn redirected(&self, _local: &Identifier, target: &RedirectTarget) -> Result<(), SubscriptionForwardingError> {
        self.owed
            .lock()
            .expect("the owed redirects are never poisoned")
            .retain(|(owed, _)| owed != target);
        Ok(())
    }

    async fn postpone_redirect(&self, _local: &Identifier, target: &RedirectTarget, after: Duration) -> Result<(), SubscriptionForwardingError> {
        let due = Instant::now() + after;
        self.owed
            .lock()
            .expect("the owed redirects are never poisoned")
            .iter_mut()
            .filter(|(owed, _)| owed == target)
            .for_each(|(_, owed_due)| *owed_due = due);
        Ok(())
    }
}
