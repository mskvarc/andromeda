//! Moving the NGSI-LD `unitCode` member between stored metadata and its column.
//!
//! `unitCode` (NGSI-LD 5.2.5) is drawn from the closed UN/CEFACT common-code
//! list, so a history row keeps it as a two-byte catalog identity instead of
//! repeating the expanded member in every metadata document. The move must be
//! exact in both directions: what the write path removes, the read path puts
//! back byte for byte, so a client that stored `unitCode` reads back the same
//! expanded instance it sent.

use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_unit_catalog::{
    unit_code_id::UnitCodeId,
    unit_code_registry::{UnitCodeSnapshotMalformed, registry},
};
use cefact_units::UnitCode;
use serde_json::{Map, Value};
use thiserror::Error;

/// JSON-LD keyword holding the code inside the expanded `unitCode` member.
const VALUE_KEYWORD: &str = "@value";

/// The `unitCode` member could not be moved between metadata and its column.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum UnitCodeMetadataError {
    /// The expanded member was not a single `@value` string entry.
    #[error("the expanded unitCode member is not a single string @value entry")]
    Malformed,
    /// The code is not one the catalog has an identity for.
    #[error("'{code}' is not a UN/CEFACT common code the unit catalog registers")]
    Unregistered {
        /// The code exactly as the client wrote it.
        code: String,
    },
    /// A stored identity names no catalog registration.
    #[error("the stored unit-code identity {identity} names no catalog registration")]
    UnknownIdentity {
        /// The identity read out of the row.
        identity: UnitCodeId,
    },
    /// The compiled-in catalog snapshot did not parse.
    #[error(transparent)]
    Catalog(#[from] UnitCodeSnapshotMalformed),
}

/// Remove the `unitCode` member from `metadata` and return its catalog identity.
///
/// Metadata that ends up empty becomes absent, so a row whose only metadata was
/// the unit stores no document at all.
///
/// # Errors
///
/// Returns [`UnitCodeMetadataError`] when the member is present but is not a
/// single `@value` string, or names a code the catalog does not register. An
/// unusable unit refuses the write rather than being dropped: silently storing
/// the attribute without its unit would change the value's meaning.
pub fn take_unit_code(metadata: &mut Option<Value>) -> Result<Option<UnitCodeId>, UnitCodeMetadataError> {
    let identity = unit_code_in(metadata.as_ref())?;
    if identity.is_none() {
        return Ok(None);
    }
    let Some(Value::Object(members)) = metadata.as_mut() else {
        return Ok(identity);
    };
    members.remove(expanded_properties::UNIT_CODE.as_str());
    if members.is_empty() {
        *metadata = None;
    }
    Ok(identity)
}

/// Read the catalog identity of the `unitCode` member without removing it.
///
/// The current-state projection carries the unit in its own column exactly as
/// history does, but it is built from the same neutral observation the history
/// row is built from and must not consume that observation's metadata.
///
/// # Errors
///
/// Returns [`UnitCodeMetadataError`] on the same terms as [`take_unit_code`]:
/// a member that is not a single `@value` string, or a code the catalog does
/// not register, refuses the write rather than being dropped.
pub fn unit_code_in(metadata: Option<&Value>) -> Result<Option<UnitCodeId>, UnitCodeMetadataError> {
    let Some(Value::Object(members)) = metadata else {
        return Ok(None);
    };
    let Some(member) = members.get(expanded_properties::UNIT_CODE.as_str()) else {
        return Ok(None);
    };
    identity_of(member).map(Some)
}

/// Put the `unitCode` member back into `metadata` as the write path found it.
///
/// A row without a stored identity leaves `metadata` untouched, which is what
/// keeps an instance that never declared a unit free of one on the way out.
///
/// # Errors
///
/// Returns [`UnitCodeMetadataError::UnknownIdentity`] when the stored identity
/// names no registration, which is catalog drift rather than client input, and
/// therefore fails the decode instead of rendering a guess.
pub fn restore_unit_code(metadata: &mut Option<Value>, identity: Option<UnitCodeId>) -> Result<(), UnitCodeMetadataError> {
    let Some(identity) = identity else {
        return Ok(());
    };
    let token = registry()?
        .entry(identity)
        .map_err(|_| UnitCodeMetadataError::UnknownIdentity { identity })?
        .token();
    let member = Value::Array(vec![Value::Object(Map::from_iter([(
        VALUE_KEYWORD.to_owned(),
        Value::String(token.as_str().to_owned()),
    )]))]);
    match metadata {
        Some(Value::Object(members)) => {
            members.insert(expanded_properties::UNIT_CODE.as_str().to_owned(), member);
        }
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) | None => {
            *metadata = Some(Value::Object(Map::from_iter([(expanded_properties::UNIT_CODE.as_str().to_owned(), member)])));
        }
    }
    Ok(())
}

/// Resolve the expanded member to the identity the catalog assigned its code.
fn identity_of(member: &Value) -> Result<UnitCodeId, UnitCodeMetadataError> {
    let Some(entries) = member.as_array() else {
        return Err(UnitCodeMetadataError::Malformed);
    };
    let [single] = entries.as_slice() else {
        return Err(UnitCodeMetadataError::Malformed);
    };
    let raw = single
        .as_object()
        .filter(|entry| entry.len() == 1)
        .and_then(|entry| entry.get(VALUE_KEYWORD))
        .and_then(Value::as_str)
        .ok_or(UnitCodeMetadataError::Malformed)?;
    let code = UnitCode::from_code(raw).ok_or_else(|| UnitCodeMetadataError::Unregistered { code: raw.to_owned() })?;
    registry()?
        .id_of(code)
        .map_err(|_| UnitCodeMetadataError::Unregistered { code: raw.to_owned() })
}

#[cfg(test)]
mod tests {
    use crate::unit_code_metadata::{UnitCodeMetadataError, restore_unit_code, take_unit_code};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use andromeda_unit_catalog::unit_code_id::UnitCodeId;
    use serde_json::{Value, json};

    fn metadata_with_unit(code: &str) -> Option<Value> {
        Some(json!({ expanded_properties::UNIT_CODE.as_str(): [{"@value": code}] }))
    }

    #[test]
    fn a_unit_bearing_instance_round_trips_through_the_column() {
        let original = metadata_with_unit("KGM");
        let mut metadata = original.clone();
        let identity = take_unit_code(&mut metadata).expect("a registered code");
        pretty_assertions::assert_eq!(metadata, None);
        assert!(identity.is_some());
        restore_unit_code(&mut metadata, identity).expect("a registered identity");
        pretty_assertions::assert_eq!(metadata, original);
    }

    #[test]
    fn sibling_metadata_members_survive_the_round_trip_unchanged() {
        let original = Some(json!({
            expanded_properties::UNIT_CODE.as_str(): [{"@value": "CEL"}],
            "https://example.org/accuracy": [{"@value": 0.5}]
        }));
        let mut metadata = original.clone();
        let identity = take_unit_code(&mut metadata).expect("a registered code");
        pretty_assertions::assert_eq!(
            metadata,
            Some(json!({"https://example.org/accuracy": [{"@value": 0.5}]})),
            "the unit member is the only one the column takes over"
        );
        restore_unit_code(&mut metadata, identity).expect("a registered identity");
        pretty_assertions::assert_eq!(metadata, original);
    }

    #[test]
    fn an_instance_without_a_unit_keeps_its_metadata_and_stores_no_identity() {
        let original = Some(json!({"https://example.org/accuracy": [{"@value": 0.5}]}));
        let mut metadata = original.clone();
        pretty_assertions::assert_eq!(take_unit_code(&mut metadata), Ok(None));
        pretty_assertions::assert_eq!(metadata, original);
        restore_unit_code(&mut metadata, None).expect("no identity to restore");
        pretty_assertions::assert_eq!(metadata, original);
    }

    #[test]
    fn an_instance_with_no_metadata_at_all_stays_absent() {
        let mut metadata: Option<Value> = None;
        pretty_assertions::assert_eq!(take_unit_code(&mut metadata), Ok(None));
        restore_unit_code(&mut metadata, None).expect("no identity to restore");
        pretty_assertions::assert_eq!(metadata, None);
    }

    #[test]
    fn an_unregistered_code_refuses_the_write() {
        let mut metadata = metadata_with_unit("NOTACODE");
        pretty_assertions::assert_eq!(
            take_unit_code(&mut metadata),
            Err(UnitCodeMetadataError::Unregistered { code: "NOTACODE".to_owned() })
        );
    }

    #[test]
    fn a_lower_case_code_refuses_the_write() {
        // Recommendation 20 common codes are upper case; accepting `kgm` would
        // make two spellings of one unit collide on different identities.
        let mut metadata = metadata_with_unit("kgm");
        pretty_assertions::assert_eq!(
            take_unit_code(&mut metadata),
            Err(UnitCodeMetadataError::Unregistered { code: "kgm".to_owned() })
        );
    }

    #[test]
    fn a_unit_member_that_is_not_a_single_value_entry_refuses_the_write() {
        let mut object = Some(json!({ expanded_properties::UNIT_CODE.as_str(): "KGM" }));
        pretty_assertions::assert_eq!(take_unit_code(&mut object), Err(UnitCodeMetadataError::Malformed));

        let mut repeated = Some(json!({ expanded_properties::UNIT_CODE.as_str(): [{"@value": "KGM"}, {"@value": "MTR"}] }));
        pretty_assertions::assert_eq!(take_unit_code(&mut repeated), Err(UnitCodeMetadataError::Malformed));

        let mut identified = Some(json!({ expanded_properties::UNIT_CODE.as_str(): [{"@id": "KGM"}] }));
        pretty_assertions::assert_eq!(take_unit_code(&mut identified), Err(UnitCodeMetadataError::Malformed));

        let mut annotated = Some(json!({ expanded_properties::UNIT_CODE.as_str(): [{"@value": "KGM", "@type": "http://www.w3.org/2001/XMLSchema#string"}] }));
        pretty_assertions::assert_eq!(take_unit_code(&mut annotated), Err(UnitCodeMetadataError::Malformed));
    }

    #[test]
    fn restoring_into_an_instance_that_stored_no_metadata_creates_the_member() {
        let mut metadata = metadata_with_unit("MTR");
        let identity = take_unit_code(&mut metadata).expect("a registered code");
        pretty_assertions::assert_eq!(metadata, None);
        restore_unit_code(&mut metadata, identity).expect("a registered identity");
        pretty_assertions::assert_eq!(metadata, metadata_with_unit("MTR"));
    }

    #[test]
    fn an_identity_the_catalog_does_not_register_fails_the_decode() {
        let identity = UnitCodeId::try_from(i16::MAX).expect("a positive identity");
        let mut metadata: Option<Value> = None;
        pretty_assertions::assert_eq!(
            restore_unit_code(&mut metadata, Some(identity)),
            Err(UnitCodeMetadataError::UnknownIdentity { identity })
        );
    }
}
