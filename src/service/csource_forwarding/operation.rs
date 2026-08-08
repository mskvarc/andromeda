//! NGSI-LD operation capability matching for context-source forwarding.

use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, CsrOperation};

/// The NGSI-LD operation being performed.
#[derive(Debug, Clone, Copy)]
pub enum ForwardingOperation {
    /// Retrieve a single entity by ID.
    RetrieveEntity,
    /// Query entities matching filter criteria.
    QueryEntity,
    /// Batch query for entities.
    QueryBatch,
    /// Create a new entity.
    CreateEntity,
    /// Delete an entity by ID.
    DeleteEntity,
    /// Merge-patch an entity.
    MergeEntity,
    /// Replace an entity entirely.
    ReplaceEntity,
    /// Append attributes to an entity.
    AppendEntityAttributes,
    /// Update existing attributes of an entity.
    UpdateEntityAttributes,
    /// Partially update a single attribute.
    PartialUpdateAttribute,
    /// Replace a single attribute.
    ReplaceEntityAttribute,
    /// Delete a single attribute from an entity.
    DeleteEntityAttribute,
    /// Batch create entities.
    CreateBatch,
    /// Batch upsert entities.
    UpsertBatch,
    /// Batch update entities.
    UpdateBatch,
    /// Batch delete entities.
    DeleteBatch,
    /// Batch merge-patch entities.
    MergeBatch,
    /// Retrieve a single temporal entity by ID.
    RetrieveTemporalEntity,
    /// Query temporal entities matching filter criteria.
    QueryTemporalEntity,
}

impl ForwardingOperation {
    /// Return whether a registration advertises this exact operation or a
    /// clause-4.20 operation group containing it.
    pub(crate) fn matches(&self, csr: &ContextSourceRegistration) -> bool {
        let ops = match &csr.access.operations {
            Some(ops) => ops,
            None => return self.is_federation_operation(),
        };
        match self {
            ForwardingOperation::RetrieveEntity => ops.iter().any(|op| {
                matches!(
                    op,
                    CsrOperation::RetrieveEntity
                        | CsrOperation::FederationOps
                        | CsrOperation::AssociationOps
                        | CsrOperation::RetrieveOps
                        | CsrOperation::RedirectionOps
                )
            }),
            ForwardingOperation::QueryEntity => ops.iter().any(|op| {
                matches!(
                    op,
                    CsrOperation::QueryEntity
                        | CsrOperation::FederationOps
                        | CsrOperation::AssociationOps
                        | CsrOperation::RetrieveOps
                        | CsrOperation::RedirectionOps
                )
            }),
            ForwardingOperation::QueryBatch => ops
                .iter()
                .any(|op| matches!(op, CsrOperation::QueryBatch | CsrOperation::FederationOps | CsrOperation::AssociationOps)),
            ForwardingOperation::CreateEntity => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::CreateEntity)),
            ForwardingOperation::DeleteEntity => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::DeleteEntity)),
            ForwardingOperation::MergeEntity => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::MergeEntity)),
            ForwardingOperation::ReplaceEntity => ops
                .iter()
                .any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::ReplaceEntity | CsrOperation::UpdateOps)),
            ForwardingOperation::AppendEntityAttributes => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::AppendAttrs)),
            ForwardingOperation::UpdateEntityAttributes => ops
                .iter()
                .any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::UpdateEntity | CsrOperation::UpdateOps)),
            ForwardingOperation::PartialUpdateAttribute => ops
                .iter()
                .any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::UpdateAttrs | CsrOperation::UpdateOps)),
            ForwardingOperation::ReplaceEntityAttribute => ops
                .iter()
                .any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::ReplaceAttrs | CsrOperation::UpdateOps)),
            ForwardingOperation::DeleteEntityAttribute => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::DeleteAttrs)),
            ForwardingOperation::CreateBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::CreateBatch)),
            ForwardingOperation::UpsertBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::UpsertBatch)),
            ForwardingOperation::UpdateBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::UpdateBatch)),
            ForwardingOperation::DeleteBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::DeleteBatch)),
            ForwardingOperation::MergeBatch => ops.iter().any(|op| matches!(op, CsrOperation::RedirectionOps | CsrOperation::MergeBatch)),
            ForwardingOperation::RetrieveTemporalEntity => ops.iter().any(|op| matches!(op, CsrOperation::RetrieveTemporal)),
            ForwardingOperation::QueryTemporalEntity => ops.iter().any(|op| matches!(op, CsrOperation::QueryTemporal)),
        }
    }

    const fn is_federation_operation(&self) -> bool {
        matches!(
            self,
            ForwardingOperation::RetrieveEntity | ForwardingOperation::QueryEntity | ForwardingOperation::QueryBatch
        )
    }
}
