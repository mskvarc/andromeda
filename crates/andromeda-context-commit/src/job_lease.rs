//! The side-effect job lease a replayed commit settles.

use crate::{lease_holder::LeaseHolder, side_effect_job_id::SideEffectJobId};

/// One durable side-effect job and the worker holding its lease.
///
/// A replay commits the job's remaining consequences and removes the job in
/// the same transaction, guarded by this lease: had the replay and the removal
/// been two transactions, a lease lost between them would let a reclaiming
/// worker replay the job again after its consequences had already landed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobLease {
    /// The job the replay settles.
    pub job: SideEffectJobId,
    /// The worker the job must still be leased to when the replay commits.
    pub holder: LeaseHolder,
}
