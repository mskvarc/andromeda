//! The relations only a temporal statement reads.
//!
//! The temporal catalogue, the history hypertable and the attribute-name
//! dictionary are named by this crate alone. They sit here rather than in the
//! shared rendering base because a temporal backend that is not PostgreSQL
//! replaces this crate, and nothing it leaves behind should still be declaring
//! a hypertable.

use sea_query::Iden;

/// The `temporal_entities` table - the entity grain temporal state hangs off.
///
/// Distinct from `entities`, and never joined to it: Core Entity deletion
/// (clause 5.6.6) removes the current-state row while the temporal evolution
/// survives until a Temporal Evolution deletion (clause 5.6.16) removes it, so
/// a temporal query that reached the entity URI or the entity types through
/// `entities` would stop answering for an entity whose history it is supposed
/// to keep serving.
#[derive(Iden, Clone, Copy)]
pub enum TemporalEntities {
    /// Table name: `temporal_entities`
    Table,
    /// Surrogate key the stream catalogue and scope history reference
    EntityKey,
    /// Entity URI, carried independently of `entities`
    Id,
    /// Array of entity type IRIs
    Types,
}

/// The `temporal_attribute_streams` table - the stable grain a history row belongs to.
#[derive(Iden, Clone, Copy)]
pub enum TemporalAttributeStreams {
    /// Table name: `temporal_attribute_streams`
    Table,
    /// Stream identifier, referenced by `attribute_history.stream_id`
    Id,
    /// Foreign key to `temporal_entities`
    TemporalEntityKey,
    /// Foreign key to `attribute_names`
    AttributeNameId,
    /// Optional dataset identifier (clause 4.5.5)
    DatasetId,
}

/// The `attribute_history` hypertable - stores historical attribute instances.
#[derive(Iden, Clone, Copy)]
pub enum AttributeHistory {
    /// Table name: `attribute_history`
    Table,
    /// Foreign key to `temporal_attribute_streams`
    StreamId,
}

/// The `attribute_names` table - normalized attribute name storage.
#[derive(Iden, Clone, Copy)]
pub enum AttributeNames {
    /// Table name: `attribute_names`
    Table,
    /// Primary key
    Id,
    /// Expanded attribute IRI
    Name,
}

/// Alias `s` for the `temporal_attribute_streams` table.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
pub struct S;

/// Alias `ah` for the `attribute_history` hypertable.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
pub struct Ah;

/// Alias `an` for the `attribute_names` table.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
pub struct An;

/// Alias `ce` for the current-state `entities` row a `q` or geo predicate reads.
///
/// A `q` expression and a geo query filter current state (clauses 4.9 and 4.10)
/// and read relations - the JSONB payload and the default `location` columns -
/// that only `entities` has. Stage 1 reaches that row through a correlated
/// `EXISTS`, never a join, so an entity whose current state has been deleted
/// keeps its history queryable and simply matches no current-state filter.
#[derive(Iden, Clone, Copy)]
#[iden = "ce"]
pub struct Ce;

/// Text of the [`Ce`] alias, for the geo fragments that are written as raw SQL
/// rather than built through sea-query and so cannot use the [`Iden`].
pub const CE_ALIAS: &str = "ce";

/// The `entity_id` name stage 1 projects the selected entity URI under.
#[derive(Iden, Clone, Copy)]
#[iden = "entity_id"]
pub struct EntityIdColumn;

#[cfg(test)]
mod tests {
    use crate::schema::{Ah, An, AttributeHistory, AttributeNames, Ce, EntityIdColumn, S, TemporalAttributeStreams, TemporalEntities};
    use sea_query::Iden;

    #[test]
    fn temporal_entity_names() {
        pretty_assertions::assert_eq!(TemporalEntities::Table.to_string(), "temporal_entities");
        pretty_assertions::assert_eq!(TemporalEntities::EntityKey.to_string(), "entity_key");
        pretty_assertions::assert_eq!(TemporalEntities::Id.to_string(), "id");
        pretty_assertions::assert_eq!(TemporalEntities::Types.to_string(), "types");
    }

    #[test]
    fn temporal_attribute_stream_names() {
        pretty_assertions::assert_eq!(TemporalAttributeStreams::Table.to_string(), "temporal_attribute_streams");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::TemporalEntityKey.to_string(), "temporal_entity_key");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::AttributeNameId.to_string(), "attribute_name_id");
        pretty_assertions::assert_eq!(TemporalAttributeStreams::DatasetId.to_string(), "dataset_id");
    }

    #[test]
    fn attribute_history_names() {
        pretty_assertions::assert_eq!(AttributeHistory::Table.to_string(), "attribute_history");
        pretty_assertions::assert_eq!(AttributeHistory::StreamId.to_string(), "stream_id");
    }

    #[test]
    fn attribute_names_names() {
        pretty_assertions::assert_eq!(AttributeNames::Table.to_string(), "attribute_names");
        pretty_assertions::assert_eq!(AttributeNames::Name.to_string(), "name");
    }

    #[test]
    fn table_aliases() {
        pretty_assertions::assert_eq!(S.to_string(), "s");
        pretty_assertions::assert_eq!(Ah.to_string(), "ah");
        pretty_assertions::assert_eq!(An.to_string(), "an");
        pretty_assertions::assert_eq!(Ce.to_string(), "ce");
        pretty_assertions::assert_eq!(EntityIdColumn.to_string(), "entity_id");
    }
}
