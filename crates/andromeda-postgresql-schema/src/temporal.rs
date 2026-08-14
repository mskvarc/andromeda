//! The relations only a temporal statement reads.
//!
//! The temporal catalogue, the history hypertable and the attribute-name
//! dictionary. `temporal_entities` is distinct from `entities`, and never
//! joined to it: Core Entity deletion (clause 5.6.6) removes the current-state
//! row while the temporal evolution survives until a Temporal Evolution
//! deletion (clause 5.6.16) removes it, so a temporal query that reached the
//! entity URI or the entity types through `entities` would stop answering for
//! an entity whose history it is supposed to keep serving.

use sea_query::Iden;

/// The `temporal_entities` table - the entity grain temporal state hangs off.
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

#[cfg(test)]
mod tests {
    use crate::temporal::{AttributeHistory, AttributeNames, TemporalAttributeStreams, TemporalEntities};
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
}
