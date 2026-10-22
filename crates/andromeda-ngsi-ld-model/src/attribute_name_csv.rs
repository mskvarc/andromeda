//! Serde support for the comma-separated attribute-name members the spec
//! defines as a single `String`.
//!
//! Table 5.2.12-1 types `expandValues` and `jsonKeys` as "String / Comma
//! separated list of attribute names", and Table 5.2.23-1 gives the `Query`
//! resource the same two members in the same shape. One encoding therefore
//! serves both resources, and it lives here rather than beside either of them.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::comma_separated_list::{ListedItem, parse_listed};
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
        .map(|raw| parse_listed(&raw, ListedItem::AttributeName).map_err(DeserializeError::custom))
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

/// The form of a comma-separated member an update fragment may clear.
///
/// Clause 5.8.3 updates a resource with a fragment: a member the fragment does
/// not mention stays, and an explicit `null` removes it, which is what a
/// [`Clearable`](crate::clearable::Clearable) member says. The carrier field
/// takes `#[serde(default)]` so that an absent member is
/// [`Clearable::Absent`](crate::clearable::Clearable::Absent).
pub mod clearable {
    use crate::{attribute_name_csv, clearable::Clearable};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde::{Deserializer, Serializer};

    /// Deserialize a member that is present: `null` clears it, a string names
    /// the list.
    ///
    /// # Errors
    ///
    /// As [`attribute_name_csv::deserialize`].
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Clearable<Vec<AttributeName>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(attribute_name_csv::deserialize(deserializer)?.map_or(Clearable::Cleared, Clearable::Set))
    }

    /// Serialize a member as the fragment wrote it: the comma-separated string,
    /// or `null` for a cleared member.
    ///
    /// # Errors
    ///
    /// Returns the serializer's own error when the member cannot be written.
    pub fn serialize<S>(value: &Clearable<Vec<AttributeName>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Clearable::Set(names) => serializer.serialize_str(&names.iter().map(AttributeName::as_str).collect::<Vec<_>>().join(",")),
            Clearable::Absent | Clearable::Cleared | Clearable::NgsiLdNull => serializer.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_name_csv, clearable::Clearable};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    /// A carrier for a member an update fragment may clear.
    #[derive(Debug, Serialize, Deserialize)]
    struct ClearableCarrier {
        #[serde(
            default,
            skip_serializing_if = "Clearable::is_absent",
            deserialize_with = "attribute_name_csv::clearable::deserialize",
            serialize_with = "attribute_name_csv::clearable::serialize"
        )]
        names: Clearable<Vec<AttributeName>>,
    }

    /// Clause 5.8.3: an absent member is left alone, a `null` clears it, and a
    /// string replaces it.
    #[test]
    fn a_clearable_member_tells_absent_from_null_from_a_list() {
        let absent: ClearableCarrier = serde_json::from_value(json!({})).expect("absent");
        assert!(absent.names.is_absent());
        let cleared: ClearableCarrier = serde_json::from_value(json!({"names": null})).expect("null");
        assert!(matches!(cleared.names, Clearable::Cleared));
        let named: ClearableCarrier = serde_json::from_value(json!({"names": "speed,colour"})).expect("a list");
        pretty_assertions::assert_eq!(
            named.names.as_set().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["speed", "colour"])
        );
        pretty_assertions::assert_eq!(serde_json::to_value(&cleared).expect("serialises"), json!({"names": null}));
        pretty_assertions::assert_eq!(serde_json::to_value(&named).expect("serialises"), json!({"names": "speed,colour"}));
        pretty_assertions::assert_eq!(serde_json::to_value(&absent).expect("serialises"), json!({}));
    }

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
