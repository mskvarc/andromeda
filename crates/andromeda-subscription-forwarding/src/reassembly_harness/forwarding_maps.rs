//! The identifier mappings of a re-assembly test: one local subscription, the
//! copy a source holds of it, and the companion that mirrors it. A copy
//! withdrawn is forgotten. A copy claimed or recorded is noted in the order it
//! was, but not added to the copies the sources hold, so the placements of one
//! test are compared off the source; only a source the scene says holds a copy
//! declines a claim.

use crate::{error::SubscriptionForwardingError, forwarded_claim::ForwardedClaim, mapping::SubscriptionForwardingStore};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_registration::removal_hook::ForwardedSubscriptionCopy;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use async_trait::async_trait;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

/// The mappings of one local subscription, which they name by `local`.
pub(crate) struct ForwardingMaps {
    /// The local subscription.
    pub(crate) local: Identifier,
    /// The registration of the source that holds a copy of it.
    pub(crate) sender: Identifier,
    /// The identifier the copy carries at that source, which its notifications
    /// are posted under.
    pub(crate) copy: Identifier,
    /// The identifier of the companion subscription linked to `local`, which
    /// a companion provisioned in place of a missing one replaces.
    companion: Mutex<Identifier>,
    /// The copies of `local` the sources hold, as `forwarded_of` answers them.
    copies: Mutex<Vec<ForwardedCopy>>,
    /// How many times the copies of `local` were asked for.
    copies_asked: AtomicUsize,
    /// The registrations whose copy mapping was removed, in the order they were.
    removed: Mutex<Vec<Identifier>>,
    /// The copies claimed before they were sent, in the order they were.
    claimed: Mutex<Vec<ForwardedCopy>>,
    /// The copies recorded under the identifier a source assigned, in the order
    /// they were.
    recorded: Mutex<Vec<ForwardedCopy>>,
}

impl ForwardingMaps {
    /// The mappings naming `local`, its `copy` held at `sender`'s source and its
    /// `companion`, with `copies` placed at the sources.
    pub(crate) const fn new(local: Identifier, sender: Identifier, copy: Identifier, companion: Identifier, copies: Vec<ForwardedCopy>) -> ForwardingMaps {
        ForwardingMaps {
            local,
            sender,
            copy,
            companion: Mutex::new(companion),
            copies: Mutex::new(copies),
            copies_asked: AtomicUsize::new(0),
            removed: Mutex::new(Vec::new()),
            claimed: Mutex::new(Vec::new()),
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

    /// The copies recorded under the identifier a source assigned.
    pub(crate) fn recorded(&self) -> Vec<ForwardedCopy> {
        self.recorded.lock().expect("the records are never poisoned").clone()
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

    async fn claim_forwarded(&self, _local: &Identifier, copy: &ForwardedCopy) -> Result<ForwardedClaim, SubscriptionForwardingError> {
        let held = self
            .copies
            .lock()
            .expect("the copies are never poisoned")
            .iter()
            .any(|held| held.registration == copy.registration);
        if held {
            return Ok(ForwardedClaim::Declined);
        }
        self.claimed.lock().expect("the claims are never poisoned").push(copy.clone());
        Ok(ForwardedClaim::Claimed)
    }

    async fn record_forwarded(&self, _local: &Identifier, copy: &ForwardedCopy) -> Result<(), SubscriptionForwardingError> {
        self.recorded.lock().expect("the records are never poisoned").push(copy.clone());
        Ok(())
    }

    async fn forwarded_of(&self, local: &Identifier) -> Result<Vec<ForwardedCopy>, SubscriptionForwardingError> {
        self.copies_asked.fetch_add(1, Ordering::SeqCst);
        Ok(if *local == self.local {
            self.copies.lock().expect("the copies are never poisoned").clone()
        } else {
            Vec::new()
        })
    }

    async fn forwarded_for_registration(&self, _registration: &Identifier) -> Result<Vec<ForwardedSubscriptionCopy>, SubscriptionForwardingError> {
        Ok(Vec::new())
    }

    async fn local_of_forwarded(&self, remote_subscription: &Identifier) -> Result<Option<(Identifier, Identifier)>, SubscriptionForwardingError> {
        Ok((*remote_subscription == self.copy).then(|| (self.local.clone(), self.sender.clone())))
    }

    async fn remove_forwarded(&self, _local: &Identifier, registration: &Identifier) -> Result<(), SubscriptionForwardingError> {
        self.copies
            .lock()
            .expect("the copies are never poisoned")
            .retain(|copy| copy.registration != *registration);
        self.removed.lock().expect("the removal record is never poisoned").push(registration.clone());
        Ok(())
    }
}
