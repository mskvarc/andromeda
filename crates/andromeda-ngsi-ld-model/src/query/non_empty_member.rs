//! Serde support for `Query` members that Table 5.2.23-1 forbids from being empty.

use andromeda_ngsi_ld_query::ordering::parameters::{OrderingBody, OrderingParameters};
use serde::{Deserialize, Deserializer, de::Error as DeserializeError};
use thiserror::Error;

/// A `Query` member that Table 5.2.23-1 requires to be non-empty and is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NonEmptyMemberError {
    /// An array member held no element.
    #[error("an empty array is not allowed")]
    EmptyArray,
    /// The `ordering` member carried no `orderBy` term.
    #[error("ordering requires a non-empty orderBy")]
    OrderingWithoutOrderBy,
}

/// Deserialize an optional array that, when present, must hold at least one
/// element ("Empty array (0 length) is not allowed", Table 5.2.23-1; a
/// violation is `BadRequestData` under clause 5.5.4).
///
/// # Errors
///
/// Returns the deserializer's own error when the member is not an array of `T`,
/// and a custom error when the array is empty.
pub fn deserialize_array<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let members = Option::<Vec<T>>::deserialize(deserializer)?;
    match members {
        Some(members) if members.is_empty() => Err(DeserializeError::custom(NonEmptyMemberError::EmptyArray)),
        other => Ok(other),
    }
}

/// Deserialize the optional `ordering` member, whose `orderBy` has cardinality
/// 1 (Table 5.2.43-1).
///
/// # Errors
///
/// Returns the deserializer's own error when the member is not a valid
/// `OrderingBody`, a custom error when its reference geometry or distance terms
/// are malformed, and a custom error when it carries no ordering terms.
pub fn deserialize_ordering<'de, D>(deserializer: D) -> Result<Option<OrderingParameters>, D::Error>
where
    D: Deserializer<'de>,
{
    let body = Option::<OrderingBody>::deserialize(deserializer)?;
    let ordering = body.map(OrderingParameters::try_from).transpose().map_err(DeserializeError::custom)?;
    match ordering {
        Some(ordering) if ordering.order_by.is_empty() => Err(DeserializeError::custom(NonEmptyMemberError::OrderingWithoutOrderBy)),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use crate::query::body::Query;
    use serde_json::json;

    fn rejected(member: &str, value: &serde_json::Value) -> bool {
        serde_json::from_value::<Query>(json!({"type": "Query", member: value})).is_err()
    }

    #[test]
    fn empty_arrays_are_rejected_for_every_restricted_member() {
        assert!(rejected("entities", &json!([])));
        assert!(rejected("attrs", &json!([])));
        assert!(rejected("pick", &json!([])));
        assert!(rejected("omit", &json!([])));
        assert!(rejected("containedBy", &json!([])));
    }

    #[test]
    fn non_empty_arrays_are_accepted() {
        assert!(serde_json::from_value::<Query>(json!({"type": "Query", "attrs": ["name"]})).is_ok());
        assert!(serde_json::from_value::<Query>(json!({"type": "Query", "entities": [{"type": "T"}]})).is_ok());
    }

    #[test]
    fn ordering_without_order_by_is_rejected() {
        assert!(rejected("ordering", &json!({"collation": "und-x-icu"})));
        assert!(rejected("ordering", &json!({"orderBy": []})));
        assert!(!rejected("ordering", &json!({"orderBy": ["name;asc"]})));
    }

    #[test]
    fn ordering_refuses_the_spellings_only_the_query_string_defines() {
        assert!(rejected("ordering", &json!({"orderBy": "name;asc"})));
        assert!(rejected("ordering", &json!({"orderBy": ["!name"]})));
        assert!(rejected("ordering", &json!({"orderBy": ["location;dist-asc"], "orderFrom": [8, 40]})));
        assert!(rejected(
            "ordering",
            &json!({"orderBy": ["location;dist-asc"], "orderGeometry": "Point", "coordinates": [8, 40]})
        ));
    }
}
