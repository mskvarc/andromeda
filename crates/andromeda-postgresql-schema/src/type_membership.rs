//! The two type-membership relations and the columns they share.
//!
//! `entity_type_members` (current state) and `temporal_entity_type_members`
//! (Temporal Evolutions) hold the same thing for two catalogues: one row per
//! type an Entity carries, keyed by type and then identifier, beside the
//! surrogate key of the catalogue row. They differ only in the table and in the
//! name of that key, so the walk a type selection reads and the rows a write
//! moves are written once, over a [`TypeMembershipRelation`].

use crate::{current::EntityTypeMembers, temporal::TemporalEntityTypeMembers};
use sea_query::{DynIden, IntoIden};

/// Which type-membership relation a statement reads or writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeMembershipRelation {
    /// `entity_type_members`, keyed by `entities.entity_key`.
    Current,
    /// `temporal_entity_type_members`, keyed by
    /// `temporal_entities.temporal_entity_key`.
    Temporal,
}

impl TypeMembershipRelation {
    /// The relation's table.
    #[must_use]
    pub fn table(self) -> DynIden {
        match self {
            TypeMembershipRelation::Current => EntityTypeMembers::Table.into_iden(),
            TypeMembershipRelation::Temporal => TemporalEntityTypeMembers::Table.into_iden(),
        }
    }

    /// The column holding the catalogue row's surrogate key.
    #[must_use]
    pub fn key(self) -> DynIden {
        match self {
            TypeMembershipRelation::Current => EntityTypeMembers::EntityKey.into_iden(),
            TypeMembershipRelation::Temporal => TemporalEntityTypeMembers::TemporalEntityKey.into_iden(),
        }
    }

    /// The column holding the `entity_type_names` key, the first column of
    /// the primary key.
    #[must_use]
    pub fn type_key(self) -> DynIden {
        match self {
            TypeMembershipRelation::Current => EntityTypeMembers::EntityTypeNameKey.into_iden(),
            TypeMembershipRelation::Temporal => TemporalEntityTypeMembers::EntityTypeNameKey.into_iden(),
        }
    }

    /// The column holding the Entity's identifier, the second column of the
    /// primary key, compared as the catalogue's own `id` is (`COLLATE "C"`).
    #[must_use]
    pub fn id(self) -> DynIden {
        match self {
            TypeMembershipRelation::Current => EntityTypeMembers::Id.into_iden(),
            TypeMembershipRelation::Temporal => TemporalEntityTypeMembers::Id.into_iden(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::type_membership::TypeMembershipRelation;
    use pretty_assertions::assert_eq;

    fn columns(relation: TypeMembershipRelation) -> [String; 4] {
        [relation.table(), relation.key(), relation.type_key(), relation.id()].map(|iden| iden.to_string())
    }

    #[test]
    fn the_current_relation_is_keyed_by_the_entity_key() {
        assert_eq!(
            columns(TypeMembershipRelation::Current),
            ["entity_type_members", "entity_key", "entity_type_name_key", "id"]
        );
    }

    #[test]
    fn the_temporal_relation_is_keyed_by_the_temporal_entity_key() {
        assert_eq!(
            columns(TypeMembershipRelation::Temporal),
            ["temporal_entity_type_members", "temporal_entity_key", "entity_type_name_key", "id"]
        );
    }
}
