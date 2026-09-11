//! The distributed operations a registration advertises (ETSI GS CIM 009 clauses 4.20 and 5.2.9).

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// CSR operation type (ETSI GS CIM 009 clause 4.20 / 5.2.9).
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum CsrOperation {
    /// Create an entity (clause 5.6.1).
    CreateEntity,
    /// Update an entity's attribute collection (clause 5.6.2).
    UpdateEntity,
    /// Append entity attributes (clause 5.6.3).
    AppendAttrs,
    /// Partially update one attribute (clause 5.6.4).
    UpdateAttrs,
    /// Delete an entity attribute (clause 5.6.5).
    DeleteAttrs,
    /// Delete an entity (clause 5.6.6).
    DeleteEntity,
    /// Batch entity creation (clause 5.6.7).
    CreateBatch,
    /// Batch entity upsert (clause 5.6.8).
    UpsertBatch,
    /// Batch entity update (clause 5.6.9).
    UpdateBatch,
    /// Batch entity deletion (clause 5.6.10).
    DeleteBatch,
    /// Create or update temporal evolution (clause 5.6.11).
    UpsertTemporal,
    /// Add attributes to temporal evolution (clause 5.6.12).
    AppendAttrsTemporal,
    /// Delete attributes from temporal evolution (clause 5.6.13).
    DeleteAttrsTemporal,
    /// Partially update a temporal attribute instance (clause 5.6.14).
    UpdateAttrInstanceTemporal,
    /// Delete a temporal attribute instance (clause 5.6.15).
    DeleteAttrInstanceTemporal,
    /// Delete temporal evolution (clause 5.6.16).
    DeleteTemporal,
    /// Merge-patch an entity (clause 5.6.17).
    MergeEntity,
    /// Replace an entity (clause 5.6.18).
    ReplaceEntity,
    /// Replace an attribute (clause 5.6.19).
    ReplaceAttrs,
    /// Batch entity merge (clause 5.6.20).
    MergeBatch,
    /// Purge entities (clause 5.6.21).
    PurgeEntity,
    /// Retrieve one entity (clause 5.7.1).
    RetrieveEntity,
    /// Query entities, excluding the batch endpoint (clause 5.7.2).
    QueryEntity,
    /// Query entities through the batch endpoint (clause 5.7.2).
    QueryBatch,
    /// Retrieve one temporal entity (clause 5.7.3).
    RetrieveTemporal,
    /// Query temporal entities (clause 5.7.4).
    QueryTemporal,
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
    /// Create a subscription (clause 5.8.1).
    CreateSubscription,
    /// Update a subscription (clause 5.8.2).
    UpdateSubscription,
    /// Retrieve a subscription (clause 5.8.3).
    RetrieveSubscription,
    /// Query subscriptions (clause 5.8.4).
    QuerySubscription,
    /// Delete a subscription (clause 5.8.5).
    DeleteSubscription,
    /// Retrieve an entity map (clause 5.14.1).
    RetrieveEntityMap,
    /// Update an entity map (clause 5.14.2).
    UpdateEntityMap,
    /// Delete an entity map (clause 5.14.3).
    DeleteEntityMap,
    /// Create an entity map for an entity query (clause 5.14.4).
    CreateEntityMapQueryEntity,
    /// Create an entity map for a temporal query (clause 5.14.5).
    CreateEntityMapQueryTemporal,
    /// Retrieve context-source identity information (clause 5.15.1).
    RetrieveContextSourceIdentity,
    /// The federation operation group from Table 4.20-2.
    FederationOps,
    /// The association operation group from Table 4.20-2.
    AssociationOps,
    /// The update operation group from Table 4.20-2.
    UpdateOps,
    /// The retrieve operation group from Table 4.20-2.
    RetrieveOps,
    /// The redirection operation group from Table 4.20-2.
    RedirectionOps,
}
