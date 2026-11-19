//! Serde support for the comma-separated attribute lists of a stored subscription, whose members are expanded attribute names.
//!
//! The wire form is the one [`attribute_name_csv`](crate::attribute_name_csv) writes, a single comma-separated `String`, and a member is an absolute IRI.

use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
use andromeda_ngsi_ld_query::comma_separated_list::{ListedMemberKind, listed_values};
use serde::{Deserialize, Deserializer, Serializer, de::Error as DeserializeError};

/// The comma-separated string that lists `names`.
#[must_use]
pub fn joined(names: &[AttributeIri]) -> String {
    names.iter().map(AttributeIri::as_str).collect::<Vec<_>>().join(",")
}

/// Deserialize a comma-separated member that is present into absolute attribute IRIs; the carrier field takes `#[serde(default)]` for an absent one.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is not a string, and a custom error when the list carries an empty member or a member that is no absolute attribute IRI.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<AttributeIri>>, D::Error>
where
    D: Deserializer<'de>,
{
    listed_values(&String::deserialize(deserializer)?, ListedMemberKind::AttributeName)
        .map(Some)
        .map_err(DeserializeError::custom)
}

/// Serialize attribute IRIs as the comma-separated member the spec defines.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize<S>(value: &Option<Vec<AttributeIri>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(names) => serializer.serialize_str(&joined(names)),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_iri_csv;
    use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier standing in for the resource that owns the member.
    #[derive(Debug, Serialize, Deserialize)]
    struct Carrier {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "attribute_iri_csv::deserialize",
            serialize_with = "attribute_iri_csv::serialize"
        )]
        names: Option<Vec<AttributeIri>>,
    }

    #[test]
    fn a_list_of_absolute_iris_round_trips_through_its_wire_string() {
        let wire = json!({"names": "https://example.com/speed,https://example.com/colour"});
        let parsed: Carrier = serde_json::from_value(wire.clone()).expect("the member parses");
        pretty_assertions::assert_eq!(parsed.names.as_ref().map(Vec::len), Some(2));
        pretty_assertions::assert_eq!(serde_json::to_value(&parsed).expect("serialises"), wire);
    }

    #[test]
    fn a_bare_term_an_empty_member_and_a_non_string_are_refused() {
        for wire in [
            json!({"names": "speed"}),
            json!({"names": "https://example.com/a,"}),
            json!({"names": ""}),
            json!({"names": 5}),
        ] {
            assert!(serde_json::from_value::<Carrier>(wire.clone()).is_err(), "{wire}");
        }
    }
}
