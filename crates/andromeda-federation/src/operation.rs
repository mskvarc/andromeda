//! NGSI-LD operation capability matching for federation (clause 4.20).

use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, CsrOperation};

/// The NGSI-LD operation being forwarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardingOperation {
    /// Retrieve a single entity by ID (clause 5.7.1).
    RetrieveEntity,
    /// Query entities matching filter criteria (clause 5.7.2).
    QueryEntity,
    /// Query entities through the batch endpoint (clause 5.7.2).
    QueryBatch,
    /// Create a new entity (clause 5.6.1).
    CreateEntity,
    /// Delete an entity by ID (clause 5.6.6).
    DeleteEntity,
    /// Merge-patch an entity (clause 5.6.17).
    MergeEntity,
    /// Replace an entity entirely (clause 5.6.18).
    ReplaceEntity,
    /// Append attributes to an entity (clause 5.6.3).
    AppendEntityAttributes,
    /// Update existing attributes of an entity (clause 5.6.2).
    UpdateEntityAttributes,
    /// Partially update a single attribute (clause 5.6.4).
    PartialUpdateAttribute,
    /// Replace a single attribute (clause 5.6.19).
    ReplaceEntityAttribute,
    /// Delete a single attribute from an entity (clause 5.6.5).
    DeleteEntityAttribute,
    /// Batch create entities (clause 5.6.7).
    CreateBatch,
    /// Batch upsert entities (clause 5.6.8).
    UpsertBatch,
    /// Batch update entities (clause 5.6.9).
    UpdateBatch,
    /// Batch delete entities (clause 5.6.10).
    DeleteBatch,
    /// Batch merge-patch entities (clause 5.6.20).
    MergeBatch,
    /// Retrieve a single temporal entity by ID (clause 5.7.3).
    RetrieveTemporalEntity,
    /// Query temporal entities matching filter criteria (clause 5.7.4).
    QueryTemporalEntity,
    /// Retrieve available entity types (clause 5.7.5).
    RetrieveEntityTypes,
    /// Retrieve details of available entity types (clause 5.7.6).
    RetrieveEntityTypeDetails,
    /// Retrieve available entity type information (clause 5.7.7).
    RetrieveEntityTypeInfo,
    /// Retrieve available attributes (clause 5.7.8).
    RetrieveAttrTypes,
    /// Retrieve details of available attributes (clause 5.7.9).
    RetrieveAttrTypeDetails,
    /// Retrieve available attribute information (clause 5.7.10).
    RetrieveAttrTypeInfo,
    /// Create a subscription at the source (clause 5.8.1).
    CreateSubscription,
    /// Update a subscription at the source (clause 5.8.2).
    UpdateSubscription,
    /// Delete a subscription at the source (clause 5.8.5).
    DeleteSubscription,
    /// Create an entity map for an entity query (clause 5.14.4).
    CreateEntityMapQueryEntity,
    /// Create an entity map for a temporal query (clause 5.14.5).
    CreateEntityMapQueryTemporal,
}

impl ForwardingOperation {
    /// The clause 4.20 tokens that advertise this operation: its own name and
    /// every named group of Table 4.20-2 that lists it.
    ///
    /// The temporal operations and the temporal entity-map creation belong to
    /// no group, so a registration advertises them only by name.
    pub(crate) const fn advertised_by(self) -> &'static [CsrOperation] {
        match self {
            ForwardingOperation::RetrieveEntity => &[
                CsrOperation::RetrieveEntity,
                CsrOperation::FederationOps,
                CsrOperation::AssociationOps,
                CsrOperation::RetrieveOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::QueryEntity => &[
                CsrOperation::QueryEntity,
                CsrOperation::FederationOps,
                CsrOperation::AssociationOps,
                CsrOperation::RetrieveOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::QueryBatch => &[CsrOperation::QueryBatch, CsrOperation::FederationOps, CsrOperation::AssociationOps],
            ForwardingOperation::CreateEntity => &[CsrOperation::CreateEntity, CsrOperation::RedirectionOps],
            ForwardingOperation::DeleteEntity => &[CsrOperation::DeleteEntity, CsrOperation::RedirectionOps],
            ForwardingOperation::MergeEntity => &[CsrOperation::MergeEntity, CsrOperation::RedirectionOps],
            ForwardingOperation::ReplaceEntity => &[CsrOperation::ReplaceEntity, CsrOperation::UpdateOps, CsrOperation::RedirectionOps],
            ForwardingOperation::AppendEntityAttributes => &[CsrOperation::AppendAttrs, CsrOperation::RedirectionOps],
            ForwardingOperation::UpdateEntityAttributes => &[CsrOperation::UpdateEntity, CsrOperation::UpdateOps, CsrOperation::RedirectionOps],
            ForwardingOperation::PartialUpdateAttribute => &[CsrOperation::UpdateAttrs, CsrOperation::UpdateOps, CsrOperation::RedirectionOps],
            ForwardingOperation::ReplaceEntityAttribute => &[CsrOperation::ReplaceAttrs, CsrOperation::UpdateOps, CsrOperation::RedirectionOps],
            ForwardingOperation::DeleteEntityAttribute => &[CsrOperation::DeleteAttrs, CsrOperation::RedirectionOps],
            ForwardingOperation::CreateBatch => &[CsrOperation::CreateBatch],
            ForwardingOperation::UpsertBatch => &[CsrOperation::UpsertBatch],
            ForwardingOperation::UpdateBatch => &[CsrOperation::UpdateBatch],
            ForwardingOperation::DeleteBatch => &[CsrOperation::DeleteBatch],
            ForwardingOperation::MergeBatch => &[CsrOperation::MergeBatch],
            ForwardingOperation::RetrieveTemporalEntity => &[CsrOperation::RetrieveTemporal],
            ForwardingOperation::QueryTemporalEntity => &[CsrOperation::QueryTemporal],
            ForwardingOperation::RetrieveEntityTypes => &[
                CsrOperation::RetrieveEntityTypes,
                CsrOperation::FederationOps,
                CsrOperation::AssociationOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::RetrieveEntityTypeDetails => &[
                CsrOperation::RetrieveEntityTypeDetails,
                CsrOperation::FederationOps,
                CsrOperation::AssociationOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::RetrieveEntityTypeInfo => &[
                CsrOperation::RetrieveEntityTypeInfo,
                CsrOperation::FederationOps,
                CsrOperation::AssociationOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::RetrieveAttrTypes => &[
                CsrOperation::RetrieveAttrTypes,
                CsrOperation::FederationOps,
                CsrOperation::AssociationOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::RetrieveAttrTypeDetails => &[
                CsrOperation::RetrieveAttrTypeDetails,
                CsrOperation::FederationOps,
                CsrOperation::AssociationOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::RetrieveAttrTypeInfo => &[
                CsrOperation::RetrieveAttrTypeInfo,
                CsrOperation::FederationOps,
                CsrOperation::AssociationOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::CreateSubscription => &[CsrOperation::CreateSubscription, CsrOperation::FederationOps, CsrOperation::AssociationOps],
            ForwardingOperation::UpdateSubscription => &[CsrOperation::UpdateSubscription, CsrOperation::FederationOps, CsrOperation::AssociationOps],
            ForwardingOperation::DeleteSubscription => &[CsrOperation::DeleteSubscription, CsrOperation::FederationOps, CsrOperation::AssociationOps],
            ForwardingOperation::CreateEntityMapQueryEntity => &[
                CsrOperation::CreateEntityMapQueryEntity,
                CsrOperation::FederationOps,
                CsrOperation::RedirectionOps,
            ],
            ForwardingOperation::CreateEntityMapQueryTemporal => &[CsrOperation::CreateEntityMapQueryTemporal],
        }
    }

    /// Whether a registration advertises this operation, by name or through a
    /// Table 4.20-2 group containing it.
    ///
    /// A registration that names no operations advertises `federationOps`
    /// (clause 4.20); the store materialises that default on write, so the
    /// fallback only serves rows written before it did.
    pub(crate) fn matches(self, csr: &ContextSourceRegistration) -> bool {
        let advertised = self.advertised_by();
        match &csr.access.operations {
            Some(operations) => operations.iter().any(|operation| advertised.contains(operation)),
            None => advertised.contains(&CsrOperation::FederationOps),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::operation::ForwardingOperation;
    use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, CsrOperation};
    use serde_json::json;

    fn registration(operations: Option<&[&str]>) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let Some(operations) = operations {
            document["operations"] = json!(operations);
        }
        serde_json::from_value(document).expect("valid registration")
    }

    #[test]
    fn every_operation_is_advertised_by_its_own_name() {
        for operation in [
            ForwardingOperation::RetrieveEntity,
            ForwardingOperation::QueryBatch,
            ForwardingOperation::CreateEntity,
            ForwardingOperation::RetrieveTemporalEntity,
            ForwardingOperation::RetrieveEntityTypes,
            ForwardingOperation::CreateSubscription,
            ForwardingOperation::CreateEntityMapQueryTemporal,
        ] {
            let own_name = operation.advertised_by()[0];
            assert!(operation.matches(&registration(Some(&[own_name.to_string().as_str()]))), "{operation:?}");
        }
    }

    #[test]
    fn table_4_20_2_groups_cover_exactly_the_operations_they_list() {
        let federation = registration(Some(&["federationOps"]));
        assert!(ForwardingOperation::QueryBatch.matches(&federation));
        assert!(ForwardingOperation::RetrieveAttrTypeInfo.matches(&federation));
        assert!(ForwardingOperation::CreateSubscription.matches(&federation));
        assert!(ForwardingOperation::CreateEntityMapQueryEntity.matches(&federation));
        assert!(!ForwardingOperation::CreateEntity.matches(&federation));
        assert!(!ForwardingOperation::QueryTemporalEntity.matches(&federation));
        assert!(!ForwardingOperation::CreateEntityMapQueryTemporal.matches(&federation));

        let association = registration(Some(&["associationOps"]));
        assert!(ForwardingOperation::QueryBatch.matches(&association));
        assert!(!ForwardingOperation::CreateEntityMapQueryEntity.matches(&association));

        let redirection = registration(Some(&["redirectionOps"]));
        assert!(ForwardingOperation::CreateEntity.matches(&redirection));
        assert!(ForwardingOperation::RetrieveEntity.matches(&redirection));
        assert!(ForwardingOperation::RetrieveEntityTypes.matches(&redirection));
        assert!(!ForwardingOperation::QueryBatch.matches(&redirection));
        assert!(!ForwardingOperation::CreateBatch.matches(&redirection));

        let retrieve = registration(Some(&["retrieveOps"]));
        assert!(ForwardingOperation::QueryEntity.matches(&retrieve));
        assert!(!ForwardingOperation::RetrieveEntityTypes.matches(&retrieve));

        let update = registration(Some(&["updateOps"]));
        assert!(ForwardingOperation::ReplaceEntityAttribute.matches(&update));
        assert!(!ForwardingOperation::AppendEntityAttributes.matches(&update));
    }

    #[test]
    fn a_registration_naming_no_operations_advertises_federation_ops() {
        let unnamed = registration(None);
        assert!(ForwardingOperation::QueryEntity.matches(&unnamed));
        assert!(!ForwardingOperation::DeleteEntity.matches(&unnamed));
    }

    #[test]
    fn every_listing_starts_with_the_operation_itself_and_carries_no_duplicate() {
        for operation in [
            ForwardingOperation::RetrieveEntity,
            ForwardingOperation::QueryEntity,
            ForwardingOperation::ReplaceEntity,
            ForwardingOperation::UpdateEntityAttributes,
            ForwardingOperation::RetrieveEntityTypeDetails,
            ForwardingOperation::UpdateSubscription,
        ] {
            let listed = operation.advertised_by();
            let first = listed[0];
            assert!(
                !matches!(
                    first,
                    CsrOperation::FederationOps
                        | CsrOperation::AssociationOps
                        | CsrOperation::UpdateOps
                        | CsrOperation::RetrieveOps
                        | CsrOperation::RedirectionOps
                ),
                "{operation:?} must list its own name first"
            );
            let mut seen = Vec::new();
            for token in listed {
                assert!(!seen.contains(token), "{operation:?} lists {token} twice");
                seen.push(*token);
            }
        }
    }
}
