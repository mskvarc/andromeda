//! The identifier mappings of a re-assembly test, with one local subscription,
//! the copy a source holds of it, and the companion that mirrors it. A copy
//! withdrawn is forgotten. A copy claimed, settled or recorded is noted in the
//! order it was.
//!
//! How a claim is kept is the scene's choice. Noted, it is not added to the
//! copies the sources hold, so the placements of one test are compared off the
//! source, and only a source the scene says holds a copy declines a claim.
//! Kept, it is a record as the store keeps one, with a lease the test ends at
//! will rather than by waiting.

use crate::{
    error::SubscriptionForwardingError,
    forwarded_claim::{ClaimOrigin, ForwardedClaim, StaleClaim},
    mapping::SubscriptionForwardingStore,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_registration::removal_hook::ForwardedSubscriptionCopy;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use async_trait::async_trait;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// How the mappings keep the copies placements claim.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaimKeeping {
    /// A claim is noted and keeps no record, so a second placement at the same
    /// source sends again.
    #[default]
    Noted,
    /// A claim keeps a record as the store does. It declines a second claim
    /// while the record is settled or under its lease, is taken over once the
    /// lease has run out, and is read back as stale until it settles.
    Kept,
}

/// Where the lease of a kept claim stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaimLease {
    /// The source's answer settled the copy.
    Settled,
    /// The answer is outstanding and the lease ends at the instant held.
    Until(Instant),
}

/// One copy of the local subscription as a kept claim records it.
#[derive(Debug, Clone)]
struct ClaimRecord {
    /// The registration whose source the copy is placed at.
    registration: Identifier,
    /// The identifier the copy is recorded under.
    remote: Identifier,
    /// Where its lease stands.
    lease: ClaimLease,
}

/// The mappings of one local subscription, which they name by `local`.
pub(crate) struct ForwardingMaps {
    /// The local subscription.
    pub(crate) local: Identifier,
    /// The registration of the source that holds a copy of it.
    pub(crate) sender: Identifier,
    /// The identifier the copy carries at that source, which its notifications
    /// are posted under.
    pub(crate) copy: Identifier,
    /// How a claim is kept.
    keeping: ClaimKeeping,
    /// The identifier of the companion subscription linked to `local`, which
    /// a companion provisioned in place of a missing one replaces.
    companion: Mutex<Identifier>,
    /// The copies of `local` the sources hold, as `forwarded_of` answers them.
    copies: Mutex<Vec<ForwardedCopy>>,
    /// The records of the claims, held only by a scene that keeps its claims.
    records: Mutex<Vec<ClaimRecord>>,
    /// How many times the copies of `local` were asked for.
    copies_asked: AtomicUsize,
    /// The registrations whose copy mapping was removed, in the order they were.
    removed: Mutex<Vec<Identifier>>,
    /// The copies claimed before they were sent, under the identifier each was
    /// sent under, in the order they were.
    claimed: Mutex<Vec<ForwardedCopy>>,
    /// The registrations whose copy was settled, in the order they were.
    settled: Mutex<Vec<Identifier>>,
    /// The copies recorded under the identifier a source assigned, in the order
    /// they were.
    recorded: Mutex<Vec<ForwardedCopy>>,
}

impl ForwardingMaps {
    /// The mappings naming `local`, its `copy` held at `sender`'s source and its
    /// `companion`, with `copies` placed at the sources and every claim kept as
    /// `keeping` says.
    pub(crate) const fn new(
        local: Identifier,
        sender: Identifier,
        copy: Identifier,
        companion: Identifier,
        copies: Vec<ForwardedCopy>,
        keeping: ClaimKeeping,
    ) -> ForwardingMaps {
        ForwardingMaps {
            local,
            sender,
            copy,
            keeping,
            companion: Mutex::new(companion),
            copies: Mutex::new(copies),
            records: Mutex::new(Vec::new()),
            copies_asked: AtomicUsize::new(0),
            removed: Mutex::new(Vec::new()),
            claimed: Mutex::new(Vec::new()),
            settled: Mutex::new(Vec::new()),
            recorded: Mutex::new(Vec::new()),
        }
    }

    /// The companion linked to the local subscription.
    pub(crate) fn companion(&self) -> Identifier {
        self.companion.lock().expect("the companion link is never poisoned").clone()
    }

    /// How many times the copies of the local subscription were asked for.
    pub(crate) fn copies_asked(&self) -> usize {
        self.copies_asked.load(Ordering::SeqCst)
    }

    /// The registrations whose copy mapping was removed, in the order they were.
    pub(crate) fn removed(&self) -> Vec<Identifier> {
        self.removed.lock().expect("the removal record is never poisoned").clone()
    }

    /// The copies claimed before they were sent, in the order they were.
    pub(crate) fn claimed(&self) -> Vec<ForwardedCopy> {
        self.claimed.lock().expect("the claims are never poisoned").clone()
    }

    /// The registrations whose copy was settled, in the order they were.
    pub(crate) fn settled(&self) -> Vec<Identifier> {
        self.settled.lock().expect("the settlements are never poisoned").clone()
    }

    /// The copies recorded under the identifier a source assigned.
    pub(crate) fn recorded(&self) -> Vec<ForwardedCopy> {
        self.recorded.lock().expect("the records are never poisoned").clone()
    }

    /// Where the lease of the copy kept at `registration` stands, `None` when
    /// no copy is kept there.
    pub(crate) fn lease_at(&self, registration: &Identifier) -> Option<ClaimLease> {
        self.records
            .lock()
            .expect("the claim records are never poisoned")
            .iter()
            .find(|record| record.registration == *registration)
            .map(|record| record.lease)
    }

    /// End the lease of every kept claim whose answer is outstanding, as the
    /// time it runs for would.
    pub(crate) fn end_claim_leases(&self) {
        let now = Instant::now();
        self.records
            .lock()
            .expect("the claim records are never poisoned")
            .iter_mut()
            .filter(|record| matches!(record.lease, ClaimLease::Until(_)))
            .for_each(|record| record.lease = ClaimLease::Until(now));
    }

    /// Note `copy` as claimed under the identifier it is sent under.
    fn note_claim(&self, copy: ForwardedCopy) {
        self.claimed.lock().expect("the claims are never poisoned").push(copy);
    }

    /// Claim `copy` against the kept records, under a lease of `lease`.
    fn claim_kept(&self, copy: &ForwardedCopy, lease: Duration) -> ForwardedClaim {
        let now = Instant::now();
        let mut records = self.records.lock().expect("the claim records are never poisoned");
        let (remote, origin) = match records.iter_mut().find(|record| record.registration == copy.registration) {
            None => {
                records.push(ClaimRecord {
                    registration: copy.registration.clone(),
                    remote: copy.remote_subscription.clone(),
                    lease: ClaimLease::Until(now + lease),
                });
                (copy.remote_subscription.clone(), ClaimOrigin::Recorded)
            }
            Some(record) => match record.lease {
                ClaimLease::Until(until) if until <= now => {
                    record.lease = ClaimLease::Until(now + lease);
                    (record.remote.clone(), ClaimOrigin::TakenOver)
                }
                ClaimLease::Until(_) | ClaimLease::Settled => return ForwardedClaim::Declined,
            },
        };
        drop(records);
        self.note_claim(ForwardedCopy {
            registration: copy.registration.clone(),
            remote_subscription: remote.clone(),
        });
        ForwardedClaim::Claimed { remote, origin }
    }
}

#[async_trait]
impl SubscriptionForwardingStore for ForwardingMaps {
    async fn link_companion(&self, _local: &Identifier, companion: &Identifier) -> Result<(), SubscriptionForwardingError> {
        *self.companion.lock().expect("the companion link is never poisoned") = companion.clone();
        Ok(())
    }

    async fn companion_of(&self, local: &Identifier) -> Result<Option<Identifier>, SubscriptionForwardingError> {
        Ok((*local == self.local).then(|| self.companion()))
    }

    async fn local_of_companion(&self, companion: &Identifier) -> Result<Option<Identifier>, SubscriptionForwardingError> {
        Ok((*companion == self.companion()).then(|| self.local.clone()))
    }

    async fn claim_forwarded(&self, _local: &Identifier, copy: &ForwardedCopy, lease: Duration) -> Result<ForwardedClaim, SubscriptionForwardingError> {
        let held = self
            .copies
            .lock()
            .expect("the copies are never poisoned")
            .iter()
            .any(|held| held.registration == copy.registration);
        if held {
            return Ok(ForwardedClaim::Declined);
        }
        Ok(match self.keeping {
            ClaimKeeping::Noted => {
                self.note_claim(copy.clone());
                ForwardedClaim::Claimed {
                    remote: copy.remote_subscription.clone(),
                    origin: ClaimOrigin::Recorded,
                }
            }
            ClaimKeeping::Kept => self.claim_kept(copy, lease),
        })
    }

    async fn settle_forwarded(&self, _local: &Identifier, registration: &Identifier) -> Result<(), SubscriptionForwardingError> {
        self.records
            .lock()
            .expect("the claim records are never poisoned")
            .iter_mut()
            .filter(|record| record.registration == *registration)
            .for_each(|record| record.lease = ClaimLease::Settled);
        self.settled.lock().expect("the settlements are never poisoned").push(registration.clone());
        Ok(())
    }

    async fn stale_claims(&self, limit: u32) -> Result<Vec<StaleClaim>, SubscriptionForwardingError> {
        let now = Instant::now();
        let mut stale: Vec<(Instant, StaleClaim)> = self
            .records
            .lock()
            .expect("the claim records are never poisoned")
            .iter()
            .filter_map(|record| match record.lease {
                ClaimLease::Until(until) if until <= now => Some((
                    until,
                    StaleClaim {
                        local: self.local.clone(),
                        registration: record.registration.clone(),
                        remote: record.remote.clone(),
                    },
                )),
                ClaimLease::Until(_) | ClaimLease::Settled => None,
            })
            .collect();
        stale.sort_by_key(|(until, _)| *until);
        Ok(stale
            .into_iter()
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .map(|(_, claim)| claim)
            .collect())
    }

    async fn record_forwarded(&self, _local: &Identifier, copy: &ForwardedCopy) -> Result<(), SubscriptionForwardingError> {
        if self.keeping == ClaimKeeping::Kept {
            let mut records = self.records.lock().expect("the claim records are never poisoned");
            records.retain(|record| record.registration != copy.registration);
            records.push(ClaimRecord {
                registration: copy.registration.clone(),
                remote: copy.remote_subscription.clone(),
                lease: ClaimLease::Settled,
            });
        }
        self.recorded.lock().expect("the records are never poisoned").push(copy.clone());
        Ok(())
    }

    async fn forwarded_of(&self, local: &Identifier) -> Result<Vec<ForwardedCopy>, SubscriptionForwardingError> {
        self.copies_asked.fetch_add(1, Ordering::SeqCst);
        if *local != self.local {
            return Ok(Vec::new());
        }
        let mut copies = self.copies.lock().expect("the copies are never poisoned").clone();
        copies.extend(
            self.records
                .lock()
                .expect("the claim records are never poisoned")
                .iter()
                .map(|record| ForwardedCopy {
                    registration: record.registration.clone(),
                    remote_subscription: record.remote.clone(),
                }),
        );
        Ok(copies)
    }

    async fn forwarded_for_registration(&self, _registration: &Identifier) -> Result<Vec<ForwardedSubscriptionCopy>, SubscriptionForwardingError> {
        Ok(Vec::new())
    }

    async fn local_of_forwarded(&self, remote_subscription: &Identifier) -> Result<Option<(Identifier, Identifier)>, SubscriptionForwardingError> {
        if *remote_subscription == self.copy {
            return Ok(Some((self.local.clone(), self.sender.clone())));
        }
        Ok(self
            .records
            .lock()
            .expect("the claim records are never poisoned")
            .iter()
            .find(|record| record.remote == *remote_subscription)
            .map(|record| (self.local.clone(), record.registration.clone())))
    }

    async fn remove_forwarded(&self, _local: &Identifier, registration: &Identifier) -> Result<(), SubscriptionForwardingError> {
        self.copies
            .lock()
            .expect("the copies are never poisoned")
            .retain(|copy| copy.registration != *registration);
        self.records
            .lock()
            .expect("the claim records are never poisoned")
            .retain(|record| record.registration != *registration);
        self.removed.lock().expect("the removal record is never poisoned").push(registration.clone());
        Ok(())
    }
}
