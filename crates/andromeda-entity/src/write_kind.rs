//! The write operations an admitted payload is bound to, as types.
//!
//! Which NGSI-LD Nulls a payload may carry and how it is normalized before
//! expansion are decided by the operation's own clause (clause 5.5.4). A
//! payload admitted for one operation is therefore only ever written by that
//! operation: the admission is typed by one of the markers below, and each
//! write path accepts only the payload admitted for it.

use crate::{
    batch::UpsertPolicy,
    write_admission::{AttributeWrite, BatchWrite, EntityWrite},
};
use andromeda_ngsi_ld_expansion::written_member::WrittenMember;
use andromeda_ngsi_ld_model::attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember};

/// A write operation whose payload is a whole Entity or an entity-shaped
/// Fragment.
pub trait EntityWriteKind: Send + Sync {
    /// The operation the marker names.
    const WRITE: EntityWrite;
    /// What the operation's payload holds under an attribute name: the
    /// members of a merge fragment for a merge (clause 5.5.12), the members of
    /// an entity otherwise.
    type Member: WrittenMember + MemberReading + Send + Sync;
}

/// A write operation whose payload is a bare Attribute Fragment.
pub trait AttributeWriteKind: Send + Sync {
    /// The operation the marker names.
    const WRITE: AttributeWrite;
    /// What the operation's Attribute Fragment is: the member of a merge
    /// fragment a partial update merges (clause 5.6.4), or the attribute a
    /// replacement writes (clause 5.6.19).
    type Member: WrittenMember + MemberReading + Send + Sync;
}

/// A batch write operation, whose payload is an array of Entities.
pub trait BatchWriteKind: Copy + Send + Sync {
    /// What each entity of the batch holds under an attribute name: the
    /// members of a merge fragment for a batch merge (clause 5.6.20), the
    /// members of an entity otherwise.
    type Member: WrittenMember + MemberReading + Send + Sync + 'static;

    /// The operation the marker names, in the mode the request selected.
    fn write(self) -> BatchWrite;
}

/// Declares one unit marker per operation and binds it to the operation's
/// variant of `$operations` through the associated `WRITE` constant of
/// `$kind`, and to the member its payload holds through the associated
/// `Member` type.
///
/// Expands to a `Copy` unit struct and one `impl $kind for $marker`, carrying
/// the doc comment written before each marker.
macro_rules! write_markers {
    ($kind:ident for $operations:ident { $($(#[$doc:meta])* $marker:ident => $variant:ident ($member:ty)),+ $(,)? }) => {
        $(
            $(#[$doc])*
            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            pub struct $marker;

            impl $kind for $marker {
                const WRITE: $operations = $operations::$variant;
                type Member = $member;
            }
        )+
    };
}

write_markers!(EntityWriteKind for EntityWrite {
    /// Create Entity (clause 5.6.1).
    CreateEntity => Create (AttributeValue),
    /// Append Attributes (clause 5.6.3).
    AppendAttributes => AppendAttributes (AttributeValue),
    /// Update Attributes (clause 5.6.2).
    UpdateAttributes => UpdateAttributes (AttributeValue),
    /// Replace Entity (clause 5.6.18).
    ReplaceEntity => ReplaceEntity (AttributeValue),
    /// Merge Entity (clause 5.6.17).
    MergeEntity => MergeEntity (MergeMember),
});

write_markers!(AttributeWriteKind for AttributeWrite {
    /// Partial Update Attribute (clause 5.6.4).
    PartialUpdateAttribute => PartialUpdate (MergeMember),
    /// Replace Attribute (clause 5.6.19).
    ReplaceAttribute => Replace (AttributeValue),
});

/// Batch Entity Creation (clause 5.6.7).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchCreate;

/// Batch Entity Update (clause 5.6.9).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchUpdate;

/// Batch Entity Merge (clause 5.6.20).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchMerge;

/// Batch Entity Creation or Update (clause 5.6.8), in the mode the request
/// selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchUpsert(pub UpsertPolicy);

impl BatchWriteKind for BatchCreate {
    type Member = AttributeValue;

    fn write(self) -> BatchWrite {
        BatchWrite::Create
    }
}

impl BatchWriteKind for BatchUpdate {
    type Member = AttributeValue;

    fn write(self) -> BatchWrite {
        BatchWrite::Update
    }
}

impl BatchWriteKind for BatchMerge {
    type Member = MergeMember;

    fn write(self) -> BatchWrite {
        BatchWrite::Merge
    }
}

impl BatchWriteKind for BatchUpsert {
    type Member = AttributeValue;

    fn write(self) -> BatchWrite {
        BatchWrite::Upsert(self.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch::UpsertPolicy,
        write_admission::{AttributeWrite, BatchWrite, EntityWrite},
        write_kind::{
            AppendAttributes,
            AttributeWriteKind,
            BatchCreate,
            BatchMerge,
            BatchUpdate,
            BatchUpsert,
            BatchWriteKind,
            CreateEntity,
            EntityWriteKind,
            MergeEntity,
            PartialUpdateAttribute,
            ReplaceAttribute,
            ReplaceEntity,
            UpdateAttributes,
        },
    };

    #[test]
    fn every_entity_write_marker_names_its_own_operation() {
        pretty_assertions::assert_eq!(CreateEntity::WRITE, EntityWrite::Create);
        pretty_assertions::assert_eq!(AppendAttributes::WRITE, EntityWrite::AppendAttributes);
        pretty_assertions::assert_eq!(UpdateAttributes::WRITE, EntityWrite::UpdateAttributes);
        pretty_assertions::assert_eq!(ReplaceEntity::WRITE, EntityWrite::ReplaceEntity);
        pretty_assertions::assert_eq!(MergeEntity::WRITE, EntityWrite::MergeEntity);
    }

    #[test]
    fn every_attribute_write_marker_names_its_own_operation() {
        pretty_assertions::assert_eq!(PartialUpdateAttribute::WRITE, AttributeWrite::PartialUpdate);
        pretty_assertions::assert_eq!(ReplaceAttribute::WRITE, AttributeWrite::Replace);
    }

    #[test]
    fn every_batch_write_marker_names_its_own_operation_and_upsert_keeps_its_mode() {
        pretty_assertions::assert_eq!(BatchCreate.write(), BatchWrite::Create);
        pretty_assertions::assert_eq!(BatchUpdate.write(), BatchWrite::Update);
        pretty_assertions::assert_eq!(BatchMerge.write(), BatchWrite::Merge);
        pretty_assertions::assert_eq!(BatchUpsert(UpsertPolicy::Update).write(), BatchWrite::Upsert(UpsertPolicy::Update));
        pretty_assertions::assert_eq!(BatchUpsert(UpsertPolicy::Replace).write(), BatchWrite::Upsert(UpsertPolicy::Replace));
    }
}
