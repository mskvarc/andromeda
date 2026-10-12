//! Moving the NGSI-LD `unitCode` member between stored metadata and its column.
//!
//! `unitCode` (NGSI-LD 5.2.5) is drawn from the closed UN/CEFACT common-code
//! list, so a history row keeps it as a two-byte catalog identity instead of
//! repeating the expanded member in every metadata document. The move must be
//! exact in both directions: what the write path removes, the read path puts
//! back, so a client that stored `unitCode` reads back the same instance it
//! sent.

use andromeda_ngsi_ld_entity::{document::slot::Slot, unit_code::UnitCode};
use andromeda_ngsi_ld_model::temporal::observation_metadata::ObservationMetadata;
use andromeda_unit_catalog::{
    expanded_unit_code::{ExpandedUnitCodeError, UnitCodeRefusal},
    unit_code_id::UnitCodeId,
    unit_code_registry::{UnitCodeSnapshotMalformed, registry},
};
use thiserror::Error;

/// The `unitCode` member could not be moved between metadata and its column.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum UnitCodeMetadataError {
    /// The member the client wrote is not a usable unit code.
    #[error(transparent)]
    Refused(#[from] UnitCodeRefusal),
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

/// Take the `unitCode` member out of `metadata`, returning what metadata
/// remains and the unit's catalog identity.
///
/// Metadata that ends up empty becomes absent, so a row whose only metadata was
/// the unit stores no document at all.
///
/// # Errors
///
/// Returns [`UnitCodeMetadataError::Refused`] when the member deletes the unit
/// with the NGSI-LD Null: that names no unit a history row could hold, and
/// dropping it would change the meaning of the value it qualifies.
pub fn take_unit_code(metadata: Option<ObservationMetadata>) -> Result<(Option<ObservationMetadata>, Option<UnitCodeId>), UnitCodeMetadataError> {
    let Some(metadata) = metadata else {
        return Ok((None, None));
    };
    match metadata.take_unit_code() {
        (remaining, None) => Ok((remaining, None)),
        (remaining, Some(Slot::Set(unit_code))) => Ok((remaining, Some(unit_code.id()))),
        (_, Some(Slot::Remove)) => Err(UnitCodeMetadataError::Refused(UnitCodeRefusal::NotACommonCode)),
    }
}

/// Put the unit `identity` names back into `metadata`, as the write path found
/// it.
///
/// A row without a stored identity leaves `metadata` as it is, which is what
/// keeps an instance that never declared a unit free of one on the way out.
///
/// # Errors
///
/// Returns [`UnitCodeMetadataError::UnknownIdentity`] when the stored identity
/// names no registration, which is catalog drift rather than client input, and
/// therefore fails the decode instead of rendering a guess.
pub fn restore_unit_code(metadata: Option<ObservationMetadata>, identity: Option<UnitCodeId>) -> Result<Option<ObservationMetadata>, UnitCodeMetadataError> {
    let Some(identity) = identity else {
        return Ok(metadata);
    };
    let token = registry()?
        .entry(identity)
        .map_err(|_| UnitCodeMetadataError::UnknownIdentity { identity })?
        .token();
    let unit_code = UnitCode::resolve(token.as_str()).map_err(|error| match error {
        ExpandedUnitCodeError::Refused(_) => UnitCodeMetadataError::UnknownIdentity { identity },
        ExpandedUnitCodeError::Catalog(malformed) => UnitCodeMetadataError::Catalog(malformed),
    })?;
    Ok(Some(ObservationMetadata::with_unit_code(metadata, unit_code)))
}

#[cfg(test)]
mod tests {
    use crate::unit_code_metadata::{UnitCodeMetadataError, restore_unit_code, take_unit_code};
    use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
    use andromeda_ngsi_ld_model::temporal::observation_metadata::ObservationMetadata;
    use andromeda_unit_catalog::{expanded_unit_code::UnitCodeRefusal, unit_code_id::UnitCodeId};
    use serde_json::{Value, json};

    /// The metadata the expanded members `stored` of a Property instance hold.
    fn metadata(stored: &Value) -> Option<ObservationMetadata> {
        let attribute: AttributeName = "https://example.org/speed".parse().expect("a valid attribute name");
        let mut instance = stored.clone(); // clone: the fixture is read as an instance declaring its family
        if let Some(members) = instance.as_object_mut() {
            members.insert("@type".to_owned(), json!([AttributeType::Property.iri_str()]));
        }
        let document = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(&instance).expect("JSON")).expect("expanded members");
        ObservationMetadata::from_parts(document.into_parts())
    }

    /// The JSON `metadata` serializes as.
    fn stored(metadata: Option<&ObservationMetadata>) -> Option<Value> {
        metadata.map(|metadata| serde_json::to_value(metadata).expect("metadata writes"))
    }

    fn with_unit(code: &str) -> Value {
        json!({ "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": code}] })
    }

    #[test]
    fn a_unit_bearing_instance_round_trips_through_the_column() {
        let (remaining, identity) = take_unit_code(metadata(&with_unit("KGM"))).expect("a registered code");
        assert!(remaining.is_none());
        assert!(identity.is_some());
        let restored = restore_unit_code(remaining, identity).expect("a registered identity");
        pretty_assertions::assert_eq!(stored(restored.as_ref()), Some(with_unit("KGM")));
    }

    #[test]
    fn sibling_metadata_members_survive_the_round_trip_unchanged() {
        let accuracy = json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.5}]}]);
        let original = json!({
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}],
            "https://example.org/accuracy": accuracy
        });
        let (remaining, identity) = take_unit_code(metadata(&original)).expect("a registered code");
        pretty_assertions::assert_eq!(
            stored(remaining.as_ref()),
            Some(json!({"https://example.org/accuracy": accuracy})),
            "the unit member is the only one the column takes over"
        );
        let restored = restore_unit_code(remaining, identity).expect("a registered identity");
        pretty_assertions::assert_eq!(stored(restored.as_ref()), Some(original));
    }

    #[test]
    fn an_instance_without_a_unit_keeps_its_metadata_and_stores_no_identity() {
        let original = json!({"https://example.org/accuracy": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.5}]}]});
        let (remaining, identity) = take_unit_code(metadata(&original)).expect("no unit to take");
        pretty_assertions::assert_eq!(identity, None);
        let restored = restore_unit_code(remaining, None).expect("no identity to restore");
        pretty_assertions::assert_eq!(stored(restored.as_ref()), Some(original));
    }

    #[test]
    fn an_instance_with_no_metadata_at_all_stays_absent() {
        pretty_assertions::assert_eq!(take_unit_code(None), Ok((None, None)));
        pretty_assertions::assert_eq!(restore_unit_code(None, None), Ok(None));
    }

    /// The NGSI-LD Null names no unit, so it refuses the write rather than
    /// being dropped.
    #[test]
    fn a_unit_deleted_with_the_null_refuses_the_write() {
        pretty_assertions::assert_eq!(
            take_unit_code(metadata(&with_unit("urn:ngsi-ld:null"))),
            Err(UnitCodeMetadataError::Refused(UnitCodeRefusal::NotACommonCode))
        );
    }

    #[test]
    fn an_identity_the_catalog_does_not_register_fails_the_decode() {
        let identity = UnitCodeId::try_from(i16::MAX).expect("a positive identity");
        pretty_assertions::assert_eq!(
            restore_unit_code(None, Some(identity)),
            Err(UnitCodeMetadataError::UnknownIdentity { identity })
        );
    }
}
