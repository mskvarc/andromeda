//! The clause 5.6.7.4 to 5.6.20.4 decision ladder: how one registration is
//! driven for a batch operation it may not advertise.

use crate::{operation::ForwardingOperation, provision_batch::BatchProvisionOperation};
use andromeda_ngsi_ld_entity::operation::selection::OverwritePolicy;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::options::UpsertMode;

/// How one registration is driven for a batch operation it may not advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FallbackPlan {
    /// The batch operation is advertised: one request carries the array.
    Batch,
    /// Only the single-entity operation is advertised: one request per entity.
    PerEntity(SingleWrite),
    /// Neither is advertised.
    NotAdvertised,
}

/// The single-entity write a batch falls back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SingleWrite {
    /// Create Entity (clause 5.6.1).
    Create,
    /// Create Entity, followed up when the source reports the entity exists
    /// (clause 5.6.8.4).
    CreateThenOnConflict(UpsertFollowUp),
    /// Replace Entity (clause 5.6.18).
    Replace,
    /// Update Attributes (clause 5.6.2).
    UpdateAttributes,
    /// Append Attributes with overwrite disabled (clause 5.6.3).
    AppendAttributesNoOverwrite,
    /// Delete Entity (clause 5.6.6).
    Delete,
    /// Merge Entity (clause 5.6.17).
    Merge,
}

/// What a batch upsert does after a source reports `AlreadyExists` on the
/// per-entity create (clause 5.6.8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpsertFollowUp {
    /// Replace Entity, the mode being replace and the operation advertised.
    Replace,
    /// Update Attributes, the mode being update and the operation advertised.
    UpdateAttributes,
    /// Nothing the mode calls for is advertised: `OperationNotSupported`.
    NotAdvertised,
}

/// The clause 5.6.7.4 to 5.6.20.4 decision ladder for one registration.
pub(super) fn plan_for(operation: BatchProvisionOperation, csr: &ContextSourceRegistration) -> FallbackPlan {
    if operation.batch_operation().matches(csr) {
        return FallbackPlan::Batch;
    }
    let advertised = |single: ForwardingOperation| single.matches(csr);
    match operation {
        BatchProvisionOperation::Create if advertised(ForwardingOperation::CreateEntity) => FallbackPlan::PerEntity(SingleWrite::Create),
        BatchProvisionOperation::Upsert(mode) if advertised(ForwardingOperation::CreateEntity) => {
            let follow_up = match mode {
                UpsertMode::Replace if advertised(ForwardingOperation::ReplaceEntity) => UpsertFollowUp::Replace,
                UpsertMode::Update if advertised(ForwardingOperation::UpdateEntityAttributes) => UpsertFollowUp::UpdateAttributes,
                UpsertMode::Replace | UpsertMode::Update => UpsertFollowUp::NotAdvertised,
            };
            FallbackPlan::PerEntity(SingleWrite::CreateThenOnConflict(follow_up))
        }
        BatchProvisionOperation::Upsert(UpsertMode::Replace) if advertised(ForwardingOperation::ReplaceEntity) => FallbackPlan::PerEntity(SingleWrite::Replace),
        BatchProvisionOperation::Upsert(UpsertMode::Update) | BatchProvisionOperation::Update(OverwritePolicy::Overwrite)
            if advertised(ForwardingOperation::UpdateEntityAttributes) =>
        {
            FallbackPlan::PerEntity(SingleWrite::UpdateAttributes)
        }
        BatchProvisionOperation::Update(OverwritePolicy::Preserve) if advertised(ForwardingOperation::AppendEntityAttributes) => {
            FallbackPlan::PerEntity(SingleWrite::AppendAttributesNoOverwrite)
        }
        BatchProvisionOperation::Delete if advertised(ForwardingOperation::DeleteEntity) => FallbackPlan::PerEntity(SingleWrite::Delete),
        BatchProvisionOperation::Merge if advertised(ForwardingOperation::MergeEntity) => FallbackPlan::PerEntity(SingleWrite::Merge),
        BatchProvisionOperation::Create
        | BatchProvisionOperation::Upsert(_)
        | BatchProvisionOperation::Update(_)
        | BatchProvisionOperation::Delete
        | BatchProvisionOperation::Merge => FallbackPlan::NotAdvertised,
    }
}

/// The clause 5.6.8.4 follow-up of a per-entity create answered `AlreadyExists`.
pub(super) const fn follow_up_write(follow_up: UpsertFollowUp) -> Option<SingleWrite> {
    match follow_up {
        UpsertFollowUp::Replace => Some(SingleWrite::Replace),
        UpsertFollowUp::UpdateAttributes => Some(SingleWrite::UpdateAttributes),
        UpsertFollowUp::NotAdvertised => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::provision_batch::{
        BatchProvisionOperation,
        fallback_plan::{FallbackPlan, SingleWrite, UpsertFollowUp, plan_for},
        registration_fixture::registration,
    };
    use andromeda_ngsi_ld_entity::operation::selection::OverwritePolicy;
    use andromeda_ngsi_ld_request::options::UpsertMode;

    #[test]
    fn the_fallback_ladder_follows_the_clause_5_6_7_4_to_5_6_20_4_decision_table() {
        let cases: Vec<(BatchProvisionOperation, &[&str], FallbackPlan)> = vec![
            (BatchProvisionOperation::Create, &["createBatch"], FallbackPlan::Batch),
            (BatchProvisionOperation::Create, &["createEntity"], FallbackPlan::PerEntity(SingleWrite::Create)),
            (BatchProvisionOperation::Create, &["retrieveEntity"], FallbackPlan::NotAdvertised),
            (BatchProvisionOperation::Upsert(UpsertMode::Replace), &["upsertBatch"], FallbackPlan::Batch),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Replace),
                &["createEntity", "replaceEntity"],
                FallbackPlan::PerEntity(SingleWrite::CreateThenOnConflict(UpsertFollowUp::Replace)),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Update),
                &["createEntity", "updateEntity"],
                FallbackPlan::PerEntity(SingleWrite::CreateThenOnConflict(UpsertFollowUp::UpdateAttributes)),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Replace),
                &["createEntity", "updateEntity"],
                FallbackPlan::PerEntity(SingleWrite::CreateThenOnConflict(UpsertFollowUp::NotAdvertised)),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Replace),
                &["replaceEntity"],
                FallbackPlan::PerEntity(SingleWrite::Replace),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Update),
                &["updateEntity"],
                FallbackPlan::PerEntity(SingleWrite::UpdateAttributes),
            ),
            (
                BatchProvisionOperation::Upsert(UpsertMode::Update),
                &["replaceEntity"],
                FallbackPlan::NotAdvertised,
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Overwrite),
                &["updateBatch"],
                FallbackPlan::Batch,
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Overwrite),
                &["updateEntity"],
                FallbackPlan::PerEntity(SingleWrite::UpdateAttributes),
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Overwrite),
                &["appendAttrs"],
                FallbackPlan::NotAdvertised,
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Preserve),
                &["appendAttrs"],
                FallbackPlan::PerEntity(SingleWrite::AppendAttributesNoOverwrite),
            ),
            (
                BatchProvisionOperation::Update(OverwritePolicy::Preserve),
                &["updateEntity"],
                FallbackPlan::NotAdvertised,
            ),
            (BatchProvisionOperation::Delete, &["deleteBatch"], FallbackPlan::Batch),
            (BatchProvisionOperation::Delete, &["deleteEntity"], FallbackPlan::PerEntity(SingleWrite::Delete)),
            (
                BatchProvisionOperation::Delete,
                &["redirectionOps"],
                FallbackPlan::PerEntity(SingleWrite::Delete),
            ),
            (BatchProvisionOperation::Merge, &["mergeBatch"], FallbackPlan::Batch),
            (BatchProvisionOperation::Merge, &["mergeEntity"], FallbackPlan::PerEntity(SingleWrite::Merge)),
            (BatchProvisionOperation::Merge, &["retrieveEntity"], FallbackPlan::NotAdvertised),
        ];
        for (operation, operations, expected) in cases {
            let csr = registration("urn:ngsi-ld:csr:1", "redirect", Some("https://example.org/Vehicle"), None, &[], operations);
            pretty_assertions::assert_eq!(plan_for(operation, &csr), expected, "{operation:?} with {operations:?}");
        }
    }
}
