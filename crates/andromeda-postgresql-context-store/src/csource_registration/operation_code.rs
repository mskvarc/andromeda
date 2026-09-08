//! The number the routing projection stores an operation as.
//!
//! The domain vocabulary of clause 4.20 and the migration-owned registry that
//! numbers it are member-for-member identical, and the translation between them
//! is still written out by hand in both directions. A blanket conversion keyed
//! on the number would let an operation a later revision of the specification
//! adds pass silently into storage as some other operation; written as
//! exhaustive matches, that addition fails to compile here, which is where the
//! decision about its persisted number belongs.
//!
//! The five group tokens of Table 4.20-2 are members in their own right on both
//! sides and are deliberately not expanded into the operations they stand for:
//! a registration that declared a group declared the group, and expanding it at
//! write time would freeze one release's reading of the table into stored rows.

use andromeda_ngsi_ld_model::registration::CsrOperation;
use andromeda_postgresql_codes::csr_operation::CsrOperationCode;

/// The registry member the projection persists for an operation.
#[must_use]
pub const fn stored_operation(operation: CsrOperation) -> CsrOperationCode {
    match operation {
        CsrOperation::CreateEntity => CsrOperationCode::CreateEntity,
        CsrOperation::UpdateEntity => CsrOperationCode::UpdateEntity,
        CsrOperation::AppendAttrs => CsrOperationCode::AppendAttrs,
        CsrOperation::UpdateAttrs => CsrOperationCode::UpdateAttrs,
        CsrOperation::DeleteAttrs => CsrOperationCode::DeleteAttrs,
        CsrOperation::DeleteEntity => CsrOperationCode::DeleteEntity,
        CsrOperation::CreateBatch => CsrOperationCode::CreateBatch,
        CsrOperation::UpsertBatch => CsrOperationCode::UpsertBatch,
        CsrOperation::UpdateBatch => CsrOperationCode::UpdateBatch,
        CsrOperation::DeleteBatch => CsrOperationCode::DeleteBatch,
        CsrOperation::UpsertTemporal => CsrOperationCode::UpsertTemporal,
        CsrOperation::AppendAttrsTemporal => CsrOperationCode::AppendAttrsTemporal,
        CsrOperation::DeleteAttrsTemporal => CsrOperationCode::DeleteAttrsTemporal,
        CsrOperation::UpdateAttrInstanceTemporal => CsrOperationCode::UpdateAttrInstanceTemporal,
        CsrOperation::DeleteAttrInstanceTemporal => CsrOperationCode::DeleteAttrInstanceTemporal,
        CsrOperation::DeleteTemporal => CsrOperationCode::DeleteTemporal,
        CsrOperation::MergeEntity => CsrOperationCode::MergeEntity,
        CsrOperation::ReplaceEntity => CsrOperationCode::ReplaceEntity,
        CsrOperation::ReplaceAttrs => CsrOperationCode::ReplaceAttrs,
        CsrOperation::MergeBatch => CsrOperationCode::MergeBatch,
        CsrOperation::PurgeEntity => CsrOperationCode::PurgeEntity,
        CsrOperation::RetrieveEntity => CsrOperationCode::RetrieveEntity,
        CsrOperation::QueryEntity => CsrOperationCode::QueryEntity,
        CsrOperation::QueryBatch => CsrOperationCode::QueryBatch,
        CsrOperation::RetrieveTemporal => CsrOperationCode::RetrieveTemporal,
        CsrOperation::QueryTemporal => CsrOperationCode::QueryTemporal,
        CsrOperation::RetrieveEntityTypes => CsrOperationCode::RetrieveEntityTypes,
        CsrOperation::RetrieveEntityTypeDetails => CsrOperationCode::RetrieveEntityTypeDetails,
        CsrOperation::RetrieveEntityTypeInfo => CsrOperationCode::RetrieveEntityTypeInfo,
        CsrOperation::RetrieveAttrTypes => CsrOperationCode::RetrieveAttrTypes,
        CsrOperation::RetrieveAttrTypeDetails => CsrOperationCode::RetrieveAttrTypeDetails,
        CsrOperation::RetrieveAttrTypeInfo => CsrOperationCode::RetrieveAttrTypeInfo,
        CsrOperation::CreateSubscription => CsrOperationCode::CreateSubscription,
        CsrOperation::UpdateSubscription => CsrOperationCode::UpdateSubscription,
        CsrOperation::RetrieveSubscription => CsrOperationCode::RetrieveSubscription,
        CsrOperation::QuerySubscription => CsrOperationCode::QuerySubscription,
        CsrOperation::DeleteSubscription => CsrOperationCode::DeleteSubscription,
        CsrOperation::RetrieveEntityMap => CsrOperationCode::RetrieveEntityMap,
        CsrOperation::UpdateEntityMap => CsrOperationCode::UpdateEntityMap,
        CsrOperation::DeleteEntityMap => CsrOperationCode::DeleteEntityMap,
        CsrOperation::CreateEntityMapQueryEntity => CsrOperationCode::CreateEntityMapQueryEntity,
        CsrOperation::CreateEntityMapQueryTemporal => CsrOperationCode::CreateEntityMapQueryTemporal,
        CsrOperation::RetrieveContextSourceIdentity => CsrOperationCode::RetrieveContextSourceIdentity,
        CsrOperation::FederationOps => CsrOperationCode::FederationOps,
        CsrOperation::AssociationOps => CsrOperationCode::AssociationOps,
        CsrOperation::UpdateOps => CsrOperationCode::UpdateOps,
        CsrOperation::RetrieveOps => CsrOperationCode::RetrieveOps,
        CsrOperation::RedirectionOps => CsrOperationCode::RedirectionOps,
    }
}

/// The operation a stored registry member names.
#[must_use]
pub const fn operation_of(code: CsrOperationCode) -> CsrOperation {
    match code {
        CsrOperationCode::CreateEntity => CsrOperation::CreateEntity,
        CsrOperationCode::UpdateEntity => CsrOperation::UpdateEntity,
        CsrOperationCode::AppendAttrs => CsrOperation::AppendAttrs,
        CsrOperationCode::UpdateAttrs => CsrOperation::UpdateAttrs,
        CsrOperationCode::DeleteAttrs => CsrOperation::DeleteAttrs,
        CsrOperationCode::DeleteEntity => CsrOperation::DeleteEntity,
        CsrOperationCode::CreateBatch => CsrOperation::CreateBatch,
        CsrOperationCode::UpsertBatch => CsrOperation::UpsertBatch,
        CsrOperationCode::UpdateBatch => CsrOperation::UpdateBatch,
        CsrOperationCode::DeleteBatch => CsrOperation::DeleteBatch,
        CsrOperationCode::UpsertTemporal => CsrOperation::UpsertTemporal,
        CsrOperationCode::AppendAttrsTemporal => CsrOperation::AppendAttrsTemporal,
        CsrOperationCode::DeleteAttrsTemporal => CsrOperation::DeleteAttrsTemporal,
        CsrOperationCode::UpdateAttrInstanceTemporal => CsrOperation::UpdateAttrInstanceTemporal,
        CsrOperationCode::DeleteAttrInstanceTemporal => CsrOperation::DeleteAttrInstanceTemporal,
        CsrOperationCode::DeleteTemporal => CsrOperation::DeleteTemporal,
        CsrOperationCode::MergeEntity => CsrOperation::MergeEntity,
        CsrOperationCode::ReplaceEntity => CsrOperation::ReplaceEntity,
        CsrOperationCode::ReplaceAttrs => CsrOperation::ReplaceAttrs,
        CsrOperationCode::MergeBatch => CsrOperation::MergeBatch,
        CsrOperationCode::PurgeEntity => CsrOperation::PurgeEntity,
        CsrOperationCode::RetrieveEntity => CsrOperation::RetrieveEntity,
        CsrOperationCode::QueryEntity => CsrOperation::QueryEntity,
        CsrOperationCode::QueryBatch => CsrOperation::QueryBatch,
        CsrOperationCode::RetrieveTemporal => CsrOperation::RetrieveTemporal,
        CsrOperationCode::QueryTemporal => CsrOperation::QueryTemporal,
        CsrOperationCode::RetrieveEntityTypes => CsrOperation::RetrieveEntityTypes,
        CsrOperationCode::RetrieveEntityTypeDetails => CsrOperation::RetrieveEntityTypeDetails,
        CsrOperationCode::RetrieveEntityTypeInfo => CsrOperation::RetrieveEntityTypeInfo,
        CsrOperationCode::RetrieveAttrTypes => CsrOperation::RetrieveAttrTypes,
        CsrOperationCode::RetrieveAttrTypeDetails => CsrOperation::RetrieveAttrTypeDetails,
        CsrOperationCode::RetrieveAttrTypeInfo => CsrOperation::RetrieveAttrTypeInfo,
        CsrOperationCode::CreateSubscription => CsrOperation::CreateSubscription,
        CsrOperationCode::UpdateSubscription => CsrOperation::UpdateSubscription,
        CsrOperationCode::RetrieveSubscription => CsrOperation::RetrieveSubscription,
        CsrOperationCode::QuerySubscription => CsrOperation::QuerySubscription,
        CsrOperationCode::DeleteSubscription => CsrOperation::DeleteSubscription,
        CsrOperationCode::RetrieveEntityMap => CsrOperation::RetrieveEntityMap,
        CsrOperationCode::UpdateEntityMap => CsrOperation::UpdateEntityMap,
        CsrOperationCode::DeleteEntityMap => CsrOperation::DeleteEntityMap,
        CsrOperationCode::CreateEntityMapQueryEntity => CsrOperation::CreateEntityMapQueryEntity,
        CsrOperationCode::CreateEntityMapQueryTemporal => CsrOperation::CreateEntityMapQueryTemporal,
        CsrOperationCode::RetrieveContextSourceIdentity => CsrOperation::RetrieveContextSourceIdentity,
        CsrOperationCode::FederationOps => CsrOperation::FederationOps,
        CsrOperationCode::AssociationOps => CsrOperation::AssociationOps,
        CsrOperationCode::UpdateOps => CsrOperation::UpdateOps,
        CsrOperationCode::RetrieveOps => CsrOperation::RetrieveOps,
        CsrOperationCode::RedirectionOps => CsrOperation::RedirectionOps,
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::operation_code::{operation_of, stored_operation};
    use andromeda_postgresql_codes::csr_operation::CsrOperationCode;

    /// Every registered number decodes to the operation whose stored number it
    /// is. The reverse direction is enforced by the compiler: both matches are
    /// exhaustive, so an operation added to either vocabulary alone does not
    /// build.
    #[test]
    fn every_operation_survives_a_round_trip_through_its_stored_number() {
        for code in CsrOperationCode::ALL {
            pretty_assertions::assert_eq!(stored_operation(operation_of(*code)), *code);
        }
    }

    /// The registry's label is the clause 4.20 token the domain enum renders,
    /// so a registration document, the domain value and the stored number all
    /// name the same operation.
    #[test]
    fn the_stored_number_of_an_operation_is_the_one_its_token_is_registered_under() {
        for code in CsrOperationCode::ALL {
            pretty_assertions::assert_eq!(code.label(), operation_of(*code).to_string());
        }
    }

    /// The two vocabularies are the same size, which is what makes the
    /// code-driven round trip above a total check of the mapping.
    #[test]
    fn the_registry_covers_the_whole_operation_vocabulary() {
        pretty_assertions::assert_eq!(CsrOperationCode::ALL.len(), 48);
    }
}
