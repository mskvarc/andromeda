//! The collation every trigram index column declares.
//!
//! `pg_trgm` extracts an indexed string's trigrams under the database's
//! default collation, and a pattern's under the collation the pattern test
//! runs under. A pattern test the index answers runs under the default, so the
//! index column declares it: under `"C"` the trigrams of a pattern leave out
//! the letters outside ASCII the default classifies, and the index drops
//! strings the pattern matches.

use crate::repository::{PinError, schema, written};
use pretty_assertions::assert_eq;

/// The trigram operator class.
const TRIGRAM_OPS: &str = "gin_trgm_ops";

#[test]
fn every_trigram_index_column_is_collated_under_the_database_default() -> Result<(), PinError> {
    let schema = schema()?;
    let trigram_indexes: Vec<(&str, String)> = schema
        .indexes()
        .iter()
        .map(|index| (index.name(), written(&schema, index.origin(), index.definition())))
        .filter(|(_, definition)| definition.contains(TRIGRAM_OPS))
        .collect();
    assert_eq!(
        trigram_indexes.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        [
            "idx_registration_information_entity_id_trgm",
            "idx_entities_id_trgm",
            "idx_ca_value_string_trgm",
            "idx_ca_value_long_text_trgm",
        ]
    );
    for (name, definition) in &trigram_indexes {
        let uncollated = definition.matches(TRIGRAM_OPS).count() - definition.matches(&format!("collate \"default\" {TRIGRAM_OPS}")).count();
        assert_eq!(uncollated, 0, "{name}: {definition}");
    }
    Ok(())
}
