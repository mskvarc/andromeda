//! Serde support for the comma-separated attribute-name members the spec
//! defines as a single `String`.
//!
//! Table 5.2.12-1 types `expandValues` and `jsonKeys` as "String / Comma
//! separated list of attribute names", and Table 5.2.23-1 gives the `Query`
//! resource the same two members in the same shape. One encoding therefore
//! serves both resources, and it lives here rather than beside either of them.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde::{Deserialize, Deserializer, Serializer, de::Error as DeserializeError};

/// Deserialize a comma-separated member into validated names.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is neither absent
/// nor a string, and a custom error when the list carries an empty member
/// or a member that does not satisfy the NGSI-LD attribute-name grammar.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<AttributeName>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .map(|name| {
                    if name.is_empty() {
                        return Err(DeserializeError::custom("attribute-name list contains an empty member"));
                    }
                    name.parse().map_err(DeserializeError::custom)
                })
                .collect()
        })
        .transpose()
}

/// Serialize validated names as the comma-separated member the spec defines.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize<S>(value: &Option<Vec<AttributeName>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(names) => serializer.serialize_str(&names.iter().map(AttributeName::as_str).collect::<Vec<_>>().join(",")),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_name_csv;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier standing in for any resource that owns one of these members.
    #[derive(Debug, Serialize, Deserialize)]
    struct Carrier {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "attribute_name_csv::deserialize",
            serialize_with = "attribute_name_csv::serialize"
        )]
        names: Option<Vec<AttributeName>>,
    }

    #[test]
    fn a_comma_separated_member_round_trips_through_its_wire_string() {
        let parsed: Carrier = serde_json::from_value(json!({"names": "speed,colour"})).expect("the member parses");
        pretty_assertions::assert_eq!(
            parsed.names.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["speed", "colour"])
        );
        pretty_assertions::assert_eq!(serde_json::to_value(&parsed).expect("serialises"), json!({"names": "speed,colour"}));
    }

    #[test]
    fn surrounding_whitespace_around_a_member_is_not_part_of_the_name() {
        let parsed: Carrier = serde_json::from_value(json!({"names": " speed , colour "})).expect("the member parses");
        pretty_assertions::assert_eq!(
            parsed.names.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["speed", "colour"])
        );
    }

    #[test]
    fn an_absent_member_stays_absent_in_both_directions() {
        let parsed: Carrier = serde_json::from_value(json!({})).expect("the member is optional");
        assert!(parsed.names.is_none());
        pretty_assertions::assert_eq!(serde_json::to_value(&parsed).expect("serialises"), json!({}));
    }

    #[test]
    fn an_empty_member_and_a_non_string_member_are_rejected() {
        assert!(serde_json::from_value::<Carrier>(json!({"names": "speed,"})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"names": ""})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"names": ["speed"]})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"names": 5})).is_err());
    }
}
