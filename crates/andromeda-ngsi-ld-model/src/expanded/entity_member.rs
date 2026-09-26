//! Readers for the entity-level members of an expanded payload: `@id`,
//! `@type`, `expiresAt` and the names of the attributes it carries. A named
//! `GeoProperty`'s geometry is read by
//! [`crate::expanded::geo_property::extract_geo_property_geometry`], the single
//! reader shared by the write/index and read/match paths, and the `scope`
//! member by [`crate::expanded::scope_member`], which owns the two readings
//! clause 4.18 gives it.

use crate::expanded::node::expanded_object_ref;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    reference::NgsiLdReferenceError,
    uri::{Uri, UriParseError},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;

/// An entity-level member of an expanded payload that cannot be read as what
/// it names.
///
/// Each reader reports the member it could not read rather than skipping it: a
/// type or identifier dropped on the way to the store would leave an entity
/// indexed under less than its payload says (clause 5.5.4).
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ExpandedMemberError {
    /// The payload carries no `@id`.
    #[error("the expanded entity carries no '@id'")]
    MissingId,
    /// The `@id` is not an identifier.
    #[error("the entity identifier '{raw}' is not a valid identifier: {source}")]
    Id {
        /// The `@id` as the payload spells it.
        raw: String,
        /// Why it is not an identifier.
        #[source]
        source: UriParseError,
    },
    /// A `@type` entry is not a string.
    #[error("an expanded entity '@type' entry is not a string")]
    TypeNotString,
    /// A `@type` entry is not an entity type name.
    #[error("the entity type '{raw}' is not a valid type name: {source}")]
    Type {
        /// The type as the payload spells it.
        raw: String,
        /// Why it is not a type name.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// A member key is not an attribute name.
    #[error("the member '{raw}' is not a valid attribute name: {source}")]
    AttributeName {
        /// The member key as the payload spells it.
        raw: String,
        /// Why it is not an attribute name.
        #[source]
        source: NgsiLdReferenceError,
    },
}

/// Extract the `@id` of an expanded entity payload.
///
/// # Errors
///
/// Returns [`ExpandedMemberError::MissingId`] when the payload carries no
/// string `@id`, and [`ExpandedMemberError::Id`] when it is not a valid
/// identifier, including one past the identifier bound.
pub fn extract_id(expanded: &Value) -> Result<Uri, ExpandedMemberError> {
    let raw = expanded_object_ref(expanded)
        .and_then(|obj| obj.get("@id"))
        .and_then(Value::as_str)
        .ok_or(ExpandedMemberError::MissingId)?;
    raw.parse().map_err(|source| ExpandedMemberError::Id { raw: raw.to_owned(), source })
}

/// Extract `@type` IRIs from an expanded entity payload.
///
/// A payload without `@type` has no types. Every entry that is present must be
/// a string naming a type: a payload member that cannot name a type is refused
/// rather than skipped, so an entity is never indexed under fewer types than
/// its payload carries.
///
/// # Errors
///
/// Returns [`ExpandedMemberError::TypeNotString`] for a `@type` that is not an
/// array of strings and [`ExpandedMemberError::Type`] for an entry that is not
/// a valid type name, including one past the identifier bound.
pub fn extract_types(expanded: &Value) -> Result<Vec<EntityTypeName>, ExpandedMemberError> {
    let Some(types) = expanded_object_ref(expanded).and_then(|obj| obj.get("@type")) else {
        return Ok(Vec::new());
    };
    types
        .as_array()
        .ok_or(ExpandedMemberError::TypeNotString)?
        .iter()
        .map(|entry| {
            let raw = entry.as_str().ok_or(ExpandedMemberError::TypeNotString)?;
            raw.parse().map_err(|source| ExpandedMemberError::Type { raw: raw.to_owned(), source })
        })
        .collect()
}

/// Extract the names of the top-level members an expanded entity payload
/// carries, in the payload map's key order: every member that is not a
/// JSON-LD keyword.
///
/// This is the key-existence reading of clause 5.5.9 `attrs`: an entity
/// matches an `attrs` term when its payload carries a member of that name,
/// which is what the write path records against each entity so the term is
/// answered by index.
///
/// # Errors
///
/// Returns [`ExpandedMemberError::AttributeName`] for a member key that is not
/// a valid attribute name, including one past the identifier bound.
pub fn extract_attribute_names(expanded: &Value) -> Result<Vec<AttributeName>, ExpandedMemberError> {
    let Some(obj) = expanded_object_ref(expanded) else {
        return Ok(Vec::new());
    };
    obj.keys()
        .filter(|key| !key.starts_with('@'))
        // clone: the error owns the refused key; only the failing path copies it.
        .map(|key| key.parse().map_err(|source| ExpandedMemberError::AttributeName { raw: key.clone(), source }))
        .collect()
}

/// Extract the `expiresAt` value from an expanded JSON-LD entity payload
/// (ETSI GS CIM 009 clause 4.22).
#[must_use]
pub fn extract_expires_at(expanded: &Value) -> Option<DateTime<Utc>> {
    let obj = expanded_object_ref(expanded)?;
    let dt_str = obj
        .get(properties::EXPIRES_AT.as_str())?
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@value"))
        .and_then(|v| v.as_str())?;
    DateTime::parse_from_rfc3339(dt_str).ok().map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use crate::expanded::entity_member::{ExpandedMemberError, extract_attribute_names, extract_expires_at, extract_id, extract_types};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier_length::IDENTIFIER_BYTE_LIMIT, uri::Uri};
    use chrono::Datelike;
    use serde_json::json;

    #[test]
    fn the_identifier_is_read_off_the_expanded_node() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001"}]);
        pretty_assertions::assert_eq!(extract_id(&entity).as_ref().map(Uri::as_str), Ok("urn:ngsi-ld:Building:001"));
    }

    #[test]
    fn an_expanded_payload_without_an_identifier_is_missing_its_identifier() {
        pretty_assertions::assert_eq!(
            extract_id(&json!([{"@type": ["https://example.com/Building"]}])),
            Err(ExpandedMemberError::MissingId)
        );
    }

    #[test]
    fn an_over_long_expanded_identifier_is_an_error_not_an_absent_one() {
        let raw = format!("urn:ngsi-ld:Building:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let error = extract_id(&json!([{"@id": raw}])).expect_err("an over-long identifier");
        assert!(matches!(error, ExpandedMemberError::Id { .. }), "{error:?}");
        assert!(error.to_string().contains("1045 bytes"), "{error}");
    }

    #[test]
    fn an_over_long_expanded_type_is_an_error_not_a_skipped_entry() {
        let long_type = format!("https://example.com/{}", "T".repeat(IDENTIFIER_BYTE_LIMIT));
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001", "@type": ["https://example.com/Building", long_type]}]);
        let error = extract_types(&entity).expect_err("an over-long type");
        assert!(matches!(error, ExpandedMemberError::Type { .. }), "{error:?}");
    }

    #[test]
    fn a_non_string_type_entry_is_an_error() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001", "@type": [7]}]);
        pretty_assertions::assert_eq!(extract_types(&entity), Err(ExpandedMemberError::TypeNotString));
    }

    #[test]
    fn an_expanded_member_key_past_the_limit_is_an_error() {
        let long_name = format!("https://example.com/{}", "n".repeat(IDENTIFIER_BYTE_LIMIT));
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001", long_name: [{"@value": 1}]}]);
        let error = extract_attribute_names(&entity).expect_err("an over-long attribute name");
        assert!(matches!(error, ExpandedMemberError::AttributeName { .. }), "{error:?}");
    }

    #[test]
    fn attribute_names_are_the_non_keyword_members_in_key_order() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "@type": ["https://example.com/Building"],
            "https://example.com/name": [{"@value": "x"}],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/a"}],
            "https://example.com/address": [{"@value": "y"}]
        }]);
        let names = extract_attribute_names(&entity).expect("readable names");
        pretty_assertions::assert_eq!(
            names.iter().map(AttributeName::as_str).collect::<Vec<&str>>(),
            vec!["https://example.com/address", "https://example.com/name", "https://uri.etsi.org/ngsi-ld/scope"]
        );
    }

    #[test]
    fn a_payload_with_only_keywords_carries_no_attribute_names() {
        pretty_assertions::assert_eq!(extract_attribute_names(&json!([])), Ok(Vec::<AttributeName>::new()));
        pretty_assertions::assert_eq!(
            extract_attribute_names(&json!({"@id": "urn:x", "@type": ["https://example.com/T"]})),
            Ok(Vec::<AttributeName>::new())
        );
    }

    #[test]
    fn extract_expires_at_from_expanded_entity() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "@type": ["https://example.com/Building"],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": "2030-12-31T23:59:59.000Z"
            }]
        }]);
        let result = extract_expires_at(&entity);
        assert!(result.is_some());
        pretty_assertions::assert_eq!(result.unwrap().year(), 2030);
    }

    #[test]
    fn extract_expires_at_missing() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "@type": ["https://example.com/Building"]
        }]);
        assert!(extract_expires_at(&entity).is_none());
    }

    #[test]
    fn extract_expires_at_invalid_date() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": "not-a-date"
            }]
        }]);
        assert!(extract_expires_at(&entity).is_none());
    }
}
