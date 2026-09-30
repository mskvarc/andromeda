//! The persisted operation set of one context-source registration.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// One operation a context source declares it serves (clauses 4.20 and 5.2.9).
    ///
    /// A registration's `operations` member is a set drawn from a fixed
    /// vocabulary, which makes it a compact code domain rather than a bag of
    /// text: the same forty-eight tokens repeat across every registration a
    /// broker holds, and a mistyped one must be refused at decode rather than
    /// forwarded to a context source that never claimed to serve it.
    ///
    /// The five group tokens of Table 4.20-2 are members in their own right and
    /// are deliberately not expanded into the operations they stand for. A
    /// registration that declares a group declared the group, and expanding it
    /// at write time would freeze one release's reading of the table into
    /// stored rows.
    ///
    /// The labels are the specification's camel-case tokens, which is what a
    /// registration document carries on the wire.
    CsrOperationCode {
        frozen_prefix: 48,
        frozen_digest: 0x2f60_2bf1_0b69_3adc,
        codes: {
            /// Create an entity (clause 5.6.1).
            CreateEntity = 1, "createEntity";
            /// Update an entity's attribute collection (clause 5.6.2).
            UpdateEntity = 2, "updateEntity";
            /// Append entity attributes (clause 5.6.3).
            AppendAttrs = 3, "appendAttrs";
            /// Partially update one attribute (clause 5.6.4).
            UpdateAttrs = 4, "updateAttrs";
            /// Delete an entity attribute (clause 5.6.5).
            DeleteAttrs = 5, "deleteAttrs";
            /// Delete an entity (clause 5.6.6).
            DeleteEntity = 6, "deleteEntity";
            /// Batch entity creation (clause 5.6.7).
            CreateBatch = 7, "createBatch";
            /// Batch entity upsert (clause 5.6.8).
            UpsertBatch = 8, "upsertBatch";
            /// Batch entity update (clause 5.6.9).
            UpdateBatch = 9, "updateBatch";
            /// Batch entity deletion (clause 5.6.10).
            DeleteBatch = 10, "deleteBatch";
            /// Create or update temporal evolution (clause 5.6.11).
            UpsertTemporal = 11, "upsertTemporal";
            /// Add attributes to temporal evolution (clause 5.6.12).
            AppendAttrsTemporal = 12, "appendAttrsTemporal";
            /// Delete attributes from temporal evolution (clause 5.6.13).
            DeleteAttrsTemporal = 13, "deleteAttrsTemporal";
            /// Partially update a temporal attribute instance (clause 5.6.14).
            UpdateAttrInstanceTemporal = 14, "updateAttrInstanceTemporal";
            /// Delete a temporal attribute instance (clause 5.6.15).
            DeleteAttrInstanceTemporal = 15, "deleteAttrInstanceTemporal";
            /// Delete temporal evolution (clause 5.6.16).
            DeleteTemporal = 16, "deleteTemporal";
            /// Merge-patch an entity (clause 5.6.17).
            MergeEntity = 17, "mergeEntity";
            /// Replace an entity (clause 5.6.18).
            ReplaceEntity = 18, "replaceEntity";
            /// Replace an attribute (clause 5.6.19).
            ReplaceAttrs = 19, "replaceAttrs";
            /// Batch entity merge (clause 5.6.20).
            MergeBatch = 20, "mergeBatch";
            /// Purge entities (clause 5.6.21).
            PurgeEntity = 21, "purgeEntity";
            /// Retrieve one entity (clause 5.7.1).
            RetrieveEntity = 22, "retrieveEntity";
            /// Query entities, excluding the batch endpoint (clause 5.7.2).
            QueryEntity = 23, "queryEntity";
            /// Query entities through the batch endpoint (clause 5.7.2).
            QueryBatch = 24, "queryBatch";
            /// Retrieve one temporal entity (clause 5.7.3).
            RetrieveTemporal = 25, "retrieveTemporal";
            /// Query temporal entities (clause 5.7.4).
            QueryTemporal = 26, "queryTemporal";
            /// Retrieve available entity types (clause 5.7.5).
            RetrieveEntityTypes = 27, "retrieveEntityTypes";
            /// Retrieve details of available entity types (clause 5.7.6).
            RetrieveEntityTypeDetails = 28, "retrieveEntityTypeDetails";
            /// Retrieve available entity type information (clause 5.7.7).
            RetrieveEntityTypeInfo = 29, "retrieveEntityTypeInfo";
            /// Retrieve available attributes (clause 5.7.8).
            RetrieveAttrTypes = 30, "retrieveAttrTypes";
            /// Retrieve details of available attributes (clause 5.7.9).
            RetrieveAttrTypeDetails = 31, "retrieveAttrTypeDetails";
            /// Retrieve available attribute information (clause 5.7.10).
            RetrieveAttrTypeInfo = 32, "retrieveAttrTypeInfo";
            /// Create a subscription (clause 5.8.1).
            CreateSubscription = 33, "createSubscription";
            /// Update a subscription (clause 5.8.2).
            UpdateSubscription = 34, "updateSubscription";
            /// Retrieve a subscription (clause 5.8.3).
            RetrieveSubscription = 35, "retrieveSubscription";
            /// Query subscriptions (clause 5.8.4).
            QuerySubscription = 36, "querySubscription";
            /// Delete a subscription (clause 5.8.5).
            DeleteSubscription = 37, "deleteSubscription";
            /// Retrieve an entity map (clause 5.14.1).
            RetrieveEntityMap = 38, "retrieveEntityMap";
            /// Update an entity map (clause 5.14.2).
            UpdateEntityMap = 39, "updateEntityMap";
            /// Delete an entity map (clause 5.14.3).
            DeleteEntityMap = 40, "deleteEntityMap";
            /// Create an entity map for an entity query (clause 5.14.4).
            CreateEntityMapQueryEntity = 41, "createEntityMapQueryEntity";
            /// Create an entity map for a temporal query (clause 5.14.5).
            CreateEntityMapQueryTemporal = 42, "createEntityMapQueryTemporal";
            /// Retrieve context-source identity information (clause 5.15.1).
            RetrieveContextSourceIdentity = 43, "retrieveContextSourceIdentity";
            /// The federation operation group of Table 4.20-2.
            FederationOps = 44, "federationOps";
            /// The association operation group of Table 4.20-2.
            AssociationOps = 45, "associationOps";
            /// The update operation group of Table 4.20-2.
            UpdateOps = 46, "updateOps";
            /// The retrieve operation group of Table 4.20-2.
            RetrieveOps = 47, "retrieveOps";
            /// The redirection operation group of Table 4.20-2.
            RedirectionOps = 48, "redirectionOps";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::csr_operation::CsrOperationCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_registry_covers_every_operation_and_group_the_specification_defines() {
        assert_eq!(CsrOperationCode::ALL.len(), 48);
    }

    #[test]
    fn the_operation_groups_are_registered_after_the_individual_operations() {
        assert_eq!(CsrOperationCode::FederationOps.code(), 44);
        assert_eq!(CsrOperationCode::RedirectionOps.code(), 48);
    }

    #[test]
    fn zero_is_not_an_operation() {
        assert!(CsrOperationCode::try_from(0).is_err());
    }

    #[test]
    fn an_operation_a_newer_release_might_add_fails_to_decode() {
        assert!(CsrOperationCode::try_from(49).is_err());
    }
}
