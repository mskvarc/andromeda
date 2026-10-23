//! The grain columns a history source projects beside its row columns.

use sea_query::Iden;

/// The columns naming the Temporal Evolution, attribute and dataset a source
/// row belongs to.
#[derive(Iden, Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryGrain {
    /// The Entity identifier, which a Temporal Evolution keeps after clause
    /// 5.6.6 deletes the Entity.
    EntityId,
    /// `attribute_names` key of the attribute.
    AttributeNameKey,
    /// Dataset identifier of the attribute (clause 4.5.5), NULL for the
    /// default instance.
    DatasetId,
    /// Recorded stream key of the grain, NULL for a grain only queued entries
    /// extend.
    StreamKey,
    /// Surrogate key of the recorded Temporal Evolution, which a source of
    /// recorded rows alone names its Evolution by in place of the identifier.
    EntityKey,
}

#[cfg(test)]
mod tests {
    use crate::history_source::grain::HistoryGrain;
    use sea_query::Iden;

    #[test]
    fn the_grain_columns_carry_the_catalogue_names() {
        let names = [
            HistoryGrain::EntityId,
            HistoryGrain::AttributeNameKey,
            HistoryGrain::DatasetId,
            HistoryGrain::StreamKey,
            HistoryGrain::EntityKey,
        ]
        .map(|column| column.to_string());
        pretty_assertions::assert_eq!(names, ["entity_id", "attribute_name_key", "dataset_id", "stream_key", "entity_key"]);
    }
}
