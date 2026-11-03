//! One round of a delivery worker: recording the settlements it hands over
//! and claiming the slots they free, in one transaction.
//!
//! The settlements are recorded first, so the claim sees the holds they take
//! and release. The branches the claim sweeps are enumerated before either: a
//! failure there fails the round before anything is recorded, and the worker
//! asks again. Once the settlements are recorded, a claim that fails rolls
//! back only its own savepoint, so the round commits the settlements and
//! answers the claim's failure beside them.

use crate::{
    error::ContextStoreError,
    notification_delivery::{
        acknowledge::record_settlements,
        branches::drainable_targets,
        claim::{ClaimTerms, ClaimedBranch, claim_due},
        claimed_event_read::ClaimDictionaries,
        rows::UnreadableClaimedDelivery,
    },
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_notification::{delivery_entry::DeliveryEntry, delivery_settlement::DeliverySettlement, held_events::HeldEvents};
use sqlx::postgres::PgPool;
use std::num::NonZeroU32;

/// The claim a round makes beside its settlements.
#[derive(Clone, Copy)]
pub(crate) struct RoundClaim<'held> {
    /// Who the claimed deliveries are leased to, for how long, and in which
    /// order they are taken.
    pub(crate) terms: ClaimTerms,
    /// The most deliveries to claim.
    pub(crate) width: NonZeroU32,
    /// The events the claiming pass already holds, handed over rather than
    /// read again; every event the claim reads is added to it.
    pub(crate) held: &'held HeldEvents,
}

/// One claimed row: the delivery it reads back as, or the leased row it could
/// not be read back as.
pub(crate) type ClaimedRow = Result<DeliveryEntry, Box<UnreadableClaimedDelivery>>;

/// What one committed round recorded and claimed.
pub(crate) struct RecordedRound {
    /// Whether each settlement was recorded under its lease, in the order
    /// given.
    pub(crate) answers: Vec<LeaseAcknowledgement>,
    /// The rows the claim leased, empty for a round that claimed nothing, or
    /// why the claim failed.
    pub(crate) claimed: Result<Vec<ClaimedRow>, ContextStoreError>,
}

/// Record every settlement of `settlements` that `worker_id` still holds the
/// lease of, then make `claim`, in one transaction.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the branches cannot be
/// enumerated, a settlement statement fails or the transaction cannot
/// commit, [`ContextStoreError::StatementBuild`] when a settlement statement
/// cannot be built, and [`ContextStoreError::StoredSnapshot`] when a stored
/// Snapshot identity cannot be read back; nothing is recorded or claimed
/// then.
pub(crate) async fn settle_and_claim(
    pool: &PgPool,
    dictionaries: ClaimDictionaries<'_>,
    worker_id: LeaseHolder,
    settlements: &[DeliverySettlement],
    claim: Option<RoundClaim<'_>>,
) -> Result<RecordedRound, ContextStoreError> {
    let mut tx = pool.begin().await?;
    let targets = match claim {
        Some(_) => drainable_targets(&mut *tx).await?,
        None => Vec::new(),
    };
    let answers = record_settlements(&mut tx, worker_id, settlements).await?;
    let branches = match claim {
        Some(claim) => claim_due(&mut tx, targets, claim.terms, claim.width, claim.held).await,
        None => Ok(Vec::new()),
    };
    tx.commit().await?;
    let claimed = match (branches, claim) {
        (Ok(branches), Some(claim)) => assemble(branches, dictionaries, claim.held).await,
        (Ok(_), None) => Ok(Vec::new()),
        (Err(error), _) => Err(error),
    };
    Ok(RecordedRound { answers, claimed })
}

/// Join every committed branch claim's rows to their events, in branch
/// order.
async fn assemble(branches: Vec<ClaimedBranch>, dictionaries: ClaimDictionaries<'_>, held: &HeldEvents) -> Result<Vec<ClaimedRow>, ContextStoreError> {
    let mut claimed = Vec::new();
    for branch in branches {
        claimed.extend(branch.assemble(dictionaries, held).await?);
    }
    Ok(claimed)
}
