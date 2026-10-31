//! What a context source's 207 Multi-Status answer to a forwarded
//! single-entity write carries (clause 6.3.6).
//!
//! Tables 6.6.3.1-2, 6.6.3.2-2, 6.7.3.1-2, 6.7.3.2-2 and 6.7.3.3-2 give the
//! five Attribute-level writes a clause 5.2.18 `UpdateResult`, while Create,
//! Purge, Delete, Replace and Merge Entity (Tables 6.4.3.1-1, 6.4.3.3-3,
//! 6.5.3.2-2, 6.5.3.3-2 and 6.5.3.4-2) answer with a clause 5.2.16
//! `BatchOperationResult`. A 207 is read
//! as the body its operation defines, so a well-formed answer of the other kind
//! is never taken for a malformed one.

use crate::operation::ForwardingOperation;

/// How the body of a 207 answer to one forwarded write is read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MultiStatusBody {
    /// A clause 5.2.18 `UpdateResult`, read once and its names expanded into
    /// the Fully Qualified Names clause 6.3.6 asks for.
    UpdateResult,
    /// Any other body, kept as the text the source wrote for the reader that
    /// decodes it.
    AsWritten,
}

impl MultiStatusBody {
    /// The body a 207 answer to a forwarded `operation` carries.
    pub(crate) const fn of(operation: ForwardingOperation) -> MultiStatusBody {
        match operation {
            ForwardingOperation::AppendEntityAttributes
            | ForwardingOperation::UpdateEntityAttributes
            | ForwardingOperation::PartialUpdateAttribute
            | ForwardingOperation::ReplaceEntityAttribute
            | ForwardingOperation::DeleteEntityAttribute => MultiStatusBody::UpdateResult,
            ForwardingOperation::RetrieveEntity
            | ForwardingOperation::QueryEntity
            | ForwardingOperation::QueryBatch
            | ForwardingOperation::CreateEntity
            | ForwardingOperation::DeleteEntity
            | ForwardingOperation::MergeEntity
            | ForwardingOperation::ReplaceEntity
            | ForwardingOperation::CreateBatch
            | ForwardingOperation::UpsertBatch
            | ForwardingOperation::UpdateBatch
            | ForwardingOperation::DeleteBatch
            | ForwardingOperation::MergeBatch
            | ForwardingOperation::PurgeEntity
            | ForwardingOperation::UpsertTemporalEntity
            | ForwardingOperation::AppendTemporalAttributes
            | ForwardingOperation::DeleteTemporalAttribute
            | ForwardingOperation::UpdateTemporalAttributeInstance
            | ForwardingOperation::DeleteTemporalAttributeInstance
            | ForwardingOperation::DeleteTemporalEntity
            | ForwardingOperation::RetrieveTemporalEntity
            | ForwardingOperation::QueryTemporalEntity
            | ForwardingOperation::RetrieveEntityTypes
            | ForwardingOperation::RetrieveEntityTypeDetails
            | ForwardingOperation::RetrieveEntityTypeInfo
            | ForwardingOperation::RetrieveAttrTypes
            | ForwardingOperation::RetrieveAttrTypeDetails
            | ForwardingOperation::RetrieveAttrTypeInfo
            | ForwardingOperation::CreateSubscription
            | ForwardingOperation::UpdateSubscription
            | ForwardingOperation::DeleteSubscription
            | ForwardingOperation::CreateEntityMapQueryEntity
            | ForwardingOperation::CreateEntityMapQueryTemporal => MultiStatusBody::AsWritten,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{multi_status_body::MultiStatusBody, operation::ForwardingOperation};

    #[test]
    fn the_five_attribute_writes_answer_207_with_an_update_result() {
        for operation in [
            ForwardingOperation::AppendEntityAttributes,
            ForwardingOperation::UpdateEntityAttributes,
            ForwardingOperation::PartialUpdateAttribute,
            ForwardingOperation::ReplaceEntityAttribute,
            ForwardingOperation::DeleteEntityAttribute,
        ] {
            pretty_assertions::assert_eq!(MultiStatusBody::of(operation), MultiStatusBody::UpdateResult, "{operation:?}");
        }
    }

    #[test]
    fn the_entity_level_writes_keep_their_207_body_as_written() {
        for operation in [
            ForwardingOperation::CreateEntity,
            ForwardingOperation::ReplaceEntity,
            ForwardingOperation::MergeEntity,
            ForwardingOperation::DeleteEntity,
            ForwardingOperation::PurgeEntity,
            ForwardingOperation::UpsertTemporalEntity,
        ] {
            pretty_assertions::assert_eq!(MultiStatusBody::of(operation), MultiStatusBody::AsWritten, "{operation:?}");
        }
    }
}
