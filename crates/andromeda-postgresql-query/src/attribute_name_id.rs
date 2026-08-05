//! The key the `attribute_names` dictionary hands out for an expanded IRI.

use derive_more::From;

/// Dictionary key for an expanded attribute IRI in the `attribute_names` table.
///
/// `attribute_names` normalises attribute IRIs to a `SMALLINT` so the temporal
/// catalogue, the history hypertable, and the `attribute_current` projection can
/// all index on a two-byte key instead of a full IRI. It is a PostgreSQL schema
/// artefact and names nothing in ETSI GS CIM 009, which is why it lives beside
/// the statements that bind it rather than in the data model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, From)]
pub struct AttributeNameId(i16);

impl AttributeNameId {
    /// The underlying `SMALLINT` value, for SQL binding.
    pub const fn get(self) -> i16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_name_id::AttributeNameId;

    #[test]
    fn a_dictionary_key_round_trips_through_its_smallint() {
        pretty_assertions::assert_eq!(AttributeNameId::from(7_i16).get(), 7_i16);
    }
}
