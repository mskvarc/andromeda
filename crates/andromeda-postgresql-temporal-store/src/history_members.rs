//! What a history row stores beside its value, keyed as current state keys it.
//!
//! An observation's metadata ([`ObservationMetadata`]) is admitted into the
//! committed model when the observation is read, so it is keyed as it is: the
//! names it mentions are minted with the rest of the statement batch's
//! ([`mention`]), then [`HistoryMembers::keyed`] makes the members a row binds.
//! A read goes the other way: the keys every row of one read holds are looked
//! up once, then [`HistoryMembers::metadata`] rebuilds the metadata.
//!
//! A sub-attribute's `createdAt` and `modifiedAt` are never stored: history
//! records the system times of the instance alone (clause 5.2.5), so every
//! element's are NULL.

use crate::{error::TemporalStoreError, stored_history_error::StoredHistoryError};
use andromeda_ngsi_ld_entity::proof_seals::ProofSeals;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, reference::NgsiLdReference, time::date_time::NgsiLdDateTime};
use andromeda_ngsi_ld_model::temporal::observation_metadata::{MetadataMembers, ObservationMetadata};
use andromeda_postgresql_attribute_instance::{
    assemble::{AssemblyError, sub_attributes},
    flatten::{FlattenError, push_elements},
    name_keys::{HeldKeys, KeyNames, MentionedNames, NameKeys},
    stored_json::StoredJson,
    sub_attribute::SubAttributeElement,
};
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use serde_json::value::to_raw_value;
use thiserror::Error;

/// The members a history row stores beside its value, every name held as its
/// dictionary key. All absent for an observation without metadata and for a
/// tombstone.
#[derive(Debug, Default, PartialEq)]
pub struct HistoryMembers {
    /// `entity_type_names` key of `valueType`.
    pub value_type_key: Option<EntityTypeNameKey>,
    /// `entity_type_names` keys of `objectType`, in written order.
    pub object_type_keys: Vec<EntityTypeNameKey>,
    /// `unitCode`.
    pub unit_code: Option<UnitCode>,
    /// `expiresAt` (clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
    /// The `entityIdSealed` and `entityTypeSealed` members, as their codec
    /// writes them.
    pub proof_seals: Option<StoredJson>,
    /// The sub-attributes as elements in pre-order, their system times NULL.
    pub sub_attributes: Vec<SubAttributeElement>,
}

/// An observation's metadata that has no stored members.
#[derive(Debug, Error)]
pub enum HistoryMembersError {
    /// A sub-attribute does not flatten: a name has no key, or a value is not
    /// in its committed spelling.
    #[error(transparent)]
    Flatten(#[from] FlattenError),
    /// A `valueType` or `objectType` IRI has no dictionary key.
    #[error("the type '{0}' has no dictionary key")]
    UnresolvedType(NgsiLdReference),
    /// The seals did not write as JSON, which the model never produces.
    #[error("the seals do not encode: {0}")]
    Seals(#[source] serde_json::Error),
}

/// Add every name the stored members of `metadata` hold as a key to
/// `mentioned`.
pub fn mention(metadata: &ObservationMetadata, mentioned: &mut MentionedNames) {
    let members = metadata.members();
    mentioned
        .types
        .extend(members.value_type.iter().chain(&members.object_types).map(EntityTypeName::from));
    mentioned.collect(&members.sub_attributes);
}

impl HistoryMembers {
    /// The members a row stores of the unit `metadata` declares alone, for a
    /// row whose other members are never keyed.
    #[must_use]
    pub fn unit_alone(metadata: Option<&ObservationMetadata>) -> HistoryMembers {
        HistoryMembers {
            unit_code: metadata.and_then(|metadata| metadata.members().unit_code),
            ..HistoryMembers::default()
        }
    }

    /// The members a row stores of `metadata`, every name held as its key in
    /// `keys`.
    ///
    /// # Errors
    ///
    /// Returns [`HistoryMembersError`] when a name has no key in `keys`, or a
    /// sub-attribute is not in its committed spelling.
    pub fn keyed(metadata: &ObservationMetadata, keys: &NameKeys) -> Result<HistoryMembers, HistoryMembersError> {
        let members = metadata.members();
        let type_key = |reference: &NgsiLdReference| keys.type_name(reference).ok_or_else(|| HistoryMembersError::UnresolvedType(reference.clone())); // clone: the refusal names the type it could not key
        let mut sub_attributes = Vec::new();
        push_elements(&members.sub_attributes, 0, keys, &mut sub_attributes)?;
        Ok(HistoryMembers {
            value_type_key: members.value_type.as_ref().map(type_key).transpose()?,
            object_type_keys: members.object_types.iter().map(type_key).collect::<Result<_, _>>()?,
            unit_code: members.unit_code,
            expires_at: members.expires_at.map(NgsiLdDateTime::get),
            proof_seals: if members.proof_seals.is_empty() {
                None
            } else {
                Some(StoredJson::new(to_raw_value(&members.proof_seals).map_err(HistoryMembersError::Seals)?))
            },
            sub_attributes,
        })
    }

    /// Add every key the members hold to `held`.
    pub fn collect_keys(&self, held: &mut HeldKeys) {
        held.types.extend(self.value_type_key.iter().chain(&self.object_type_keys).copied());
        held.collect_elements(&self.sub_attributes);
    }

    /// The metadata of an observation of `attribute`, every key read back
    /// through `names`; `None` when the row stores none.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::StoredHistory`] when a key has no name in
    /// `names`, the elements break an invariant of the stored layout, or the
    /// seals do not read back.
    pub fn metadata(self, attribute: &AttributeName, names: &KeyNames) -> Result<Option<ObservationMetadata>, TemporalStoreError> {
        let HistoryMembers {
            value_type_key,
            object_type_keys,
            unit_code,
            expires_at,
            proof_seals,
            sub_attributes: elements,
        } = self;
        let type_name = |key: EntityTypeNameKey| names.type_name(key).cloned().ok_or(AssemblyError::UnknownType(key)); // clone: the metadata owns its type names
        let value_type = value_type_key.map(type_name).transpose().map_err(stored_members)?;
        let object_types = object_type_keys
            .into_iter()
            .map(type_name)
            .collect::<Result<Vec<_>, _>>()
            .map_err(stored_members)?;
        let proof_seals = proof_seals
            .map(|seals| ProofSeals::from_stored(attribute, seals.get()))
            .transpose()
            .map_err(|refusal| StoredHistoryError::ProofSeals(Box::new(refusal)))?
            .unwrap_or_default();
        let expires_at = expires_at.map(NgsiLdDateTime::normalise).transpose().map_err(StoredHistoryError::ExpiresAt)?;
        Ok(ObservationMetadata::new(MetadataMembers {
            expires_at,
            unit_code,
            value_type,
            object_types,
            proof_seals,
            sub_attributes: sub_attributes(elements, names).map_err(stored_members)?,
        }))
    }
}

/// The store fault of members that do not assemble.
fn stored_members(error: AssemblyError) -> StoredHistoryError {
    StoredHistoryError::Members(Box::new(error))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::history_members::{HistoryMembers, mention};
    use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
    use andromeda_ngsi_ld_model::temporal::observation_metadata::ObservationMetadata;
    use andromeda_postgresql_attribute_instance::{
        name_keys::{KeyNames, MentionedNames, NameKeys},
        name_resolution::distinct,
    };
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
    };
    use serde_json::{Value, json};

    fn ngsi(term: &str) -> String {
        format!("https://uri.etsi.org/ngsi-ld/{term}")
    }

    fn ex(term: &str) -> String {
        format!("https://ex.org/{term}")
    }

    fn date_time(text: &str) -> Value {
        json!([{"@value": text, "@type": ngsi("DateTime")}])
    }

    /// `instance` with the members of `extra` added.
    fn with(mut instance: Value, extra: &Value) -> Value {
        if let (Some(members), Some(extra)) = (instance.as_object_mut(), extra.as_object()) {
            members.extend(extra.iter().map(|(name, value)| (name.clone(), value.clone()))); // clone: the fixture owns its members
        }
        instance
    }

    /// One sub-attribute of every family, each holding `nested`.
    fn every_family(nested: &Value) -> Value {
        let property = |family: &str, members: Value| json!([with(with(json!({"@type": [ngsi(family)]}), &members), nested)]);
        json!({
            ex("accuracy"): property("Property", json!({
                ngsi("hasValue"): [{"@value": 0.5}],
                ngsi("unitCode"): [{"@value": "MTR"}],
                ngsi("observedAt"): date_time("2026-01-01T00:00:00.500Z")
            })),
            ex("where"): property("GeoProperty", json!({
                ngsi("hasValue"): [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 13.5}, {"@value": 52.25}]}],
                    "https://purl.org/geojson/vocab#bbox": [{"@list": [{"@value": 13.5}, {"@value": 52.25}, {"@value": 13.5}, {"@value": 52.25}]}]
                }]
            })),
            ex("source"): property("Relationship", json!({
                ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Sensor:1"}],
                ngsi("hasObjectType"): [{"@id": ex("Sensor")}]
            })),
            ex("label"): property("LanguageProperty", json!({
                ngsi("hasLanguageMap"): [{"@value": "schnell", "@language": "de"}, {"@value": "fast", "@language": "en"}]
            })),
            ex("mode"): property("VocabProperty", json!({ngsi("hasVocab"): [{"@id": ex("cruise")}]})),
            ex("raw"): property("JsonProperty", json!({ngsi("hasJSON"): [{"@type": "@json", "@value": {"a": [1, 2.5]}}]})),
            ex("steps"): property("ListProperty", json!({ngsi("hasValueList"): [{"@list": [{"@value": 1}, {"@value": "two"}]}]})),
            ex("stops"): property("ListRelationship", json!({
                ngsi("hasObjectList"): [{"@list": [{ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Stop:1"}]}]}]
            }))
        })
    }

    /// Every family two levels deep: each sub-attribute of [`every_family`]
    /// holds a `method` sub-attribute of its own.
    fn nested_every_family() -> Value {
        every_family(&json!({ex("method"): [{"@type": [ngsi("Property")], ngsi("hasValue"): [{"@value": "gps"}]}]}))
    }

    /// The metadata of an instance of `attribute`, of `family`, carrying
    /// `members` beside its value.
    fn metadata(attribute: &str, family: &str, members: &Value) -> (AttributeName, ObservationMetadata) {
        let attribute: AttributeName = attribute.parse().expect("an attribute name");
        let instance = with(json!({"@type": [ngsi(family)]}), members);
        let document = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(&instance).expect("JSON")).expect("expanded members");
        let metadata = ObservationMetadata::admit(document.into_parts())
            .expect("the members are committed")
            .expect("the members are metadata");
        (attribute, metadata)
    }

    /// Keys for every name `metadata` mentions, numbered from 1 in sorted
    /// order, and the names behind them.
    fn dictionary(metadata: &ObservationMetadata) -> (NameKeys, KeyNames) {
        let mut mentioned = MentionedNames::default();
        mention(metadata, &mut mentioned);
        let MentionedNames { attributes, types } = distinct(mentioned);
        let attributes: Vec<(AttributeName, AttributeNameKey)> =
            attributes.into_iter().zip(1..).map(|(name, key)| (name, AttributeNameKey::from(key))).collect();
        let types: Vec<_> = types.into_iter().zip(1..).map(|(name, key)| (name, EntityTypeNameKey::from(key))).collect();
        let names = KeyNames::from_pairs(
            attributes.iter().map(|(name, key)| (*key, name.clone())), // clone: both directions own the names
            types.iter().map(|(name, key)| (*key, NgsiLdReference::from(name))),
        );
        (NameKeys::new(ResolvedNames::from_pairs(attributes), ResolvedNames::from_pairs(types)), names)
    }

    /// The members `metadata` of `attribute` stores, and the metadata they read
    /// back as.
    fn round_trip(attribute: &AttributeName, metadata: &ObservationMetadata) -> (HistoryMembers, Option<ObservationMetadata>) {
        let (keys, names) = dictionary(metadata);
        let members = HistoryMembers::keyed(metadata, &keys).expect("every name has a key");
        let stored = HistoryMembers::keyed(metadata, &keys).expect("every name has a key");
        let read = stored.metadata(attribute, &names).expect("the members assemble");
        (members, read)
    }

    /// The expanded members `metadata` renders as.
    fn rendered(metadata: Option<&ObservationMetadata>) -> Value {
        serde_json::to_value(metadata.cloned().map(InstanceDocument::from)).expect("metadata writes") // clone: the fixture is compared after it is rendered
    }

    #[test]
    fn observation_metadata_round_trips_through_the_typed_columns() {
        let members = with(
            json!({
                ngsi("hasValue"): [{"@value": 55.5}],
                ngsi("hasValueType"): [{"@id": ex("Speed")}],
                ngsi("unitCode"): [{"@value": "KMH"}],
                ngsi("expiresAt"): date_time("2030-01-01T00:00:00Z"),
                ngsi("entityIdSealed"): [{"@value": "urn:ngsi-ld:Vehicle:1"}],
                ngsi("entityTypeSealed"): [{"@value": "Vehicle"}]
            }),
            &nested_every_family(),
        );
        let (speed, written) = metadata(&ex("speed"), "Property", &members);
        let (stored, read) = round_trip(&speed, &written);
        assert!(stored.value_type_key.is_some() && stored.unit_code.is_some() && stored.proof_seals.is_some() && stored.expires_at.is_some());
        pretty_assertions::assert_eq!(stored.sub_attributes.len(), 16, "eight families, each with its method");
        pretty_assertions::assert_eq!(rendered(read.as_ref()), rendered(Some(&written)));

        let owner = json!({
            ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Person:1"}],
            ngsi("hasObjectType"): [{"@id": ex("A")}, {"@id": ex("B")}]
        });
        let (owner_name, written) = metadata(&ex("owner"), "Relationship", &owner);
        let (stored, read) = round_trip(&owner_name, &written);
        pretty_assertions::assert_eq!(stored.object_type_keys.len(), 2);
        pretty_assertions::assert_eq!(rendered(read.as_ref()), rendered(Some(&written)));
    }

    /// Clause 5.2.5: history records the system times of the instance alone.
    #[test]
    fn sub_attribute_system_times_are_not_stored() {
        let stamped = json!({
            ngsi("hasValue"): [{"@value": 1}],
            ex("accuracy"): [{
                "@type": [ngsi("Property")],
                ngsi("hasValue"): [{"@value": 0.5}],
                ngsi("createdAt"): date_time("2024-01-01T00:00:00Z"),
                ngsi("modifiedAt"): date_time("2024-01-02T00:00:00Z"),
                ex("method"): [{
                    "@type": [ngsi("Property")],
                    ngsi("hasValue"): [{"@value": "gps"}],
                    ngsi("createdAt"): date_time("2024-01-01T00:00:00Z")
                }]
            }]
        });
        let (speed, written) = metadata(&ex("speed"), "Property", &stamped);
        let (stored, _) = round_trip(&speed, &written);
        pretty_assertions::assert_eq!(stored.sub_attributes.len(), 2);
        assert!(
            stored
                .sub_attributes
                .iter()
                .all(|element| element.created_at.is_none() && element.modified_at.is_none()),
            "{:?}",
            stored.sub_attributes
        );
    }

    #[test]
    fn an_observation_without_metadata_stores_no_element() {
        let speed: AttributeName = ex("speed").parse().expect("an attribute name");
        let stored = HistoryMembers::unit_alone(None);
        pretty_assertions::assert_eq!(stored, HistoryMembers::default());
        assert!(stored.metadata(&speed, &KeyNames::default()).expect("nothing to assemble").is_none());
    }

    /// A row whose other members are never keyed keeps the unit alone.
    #[test]
    fn the_unit_alone_is_the_unit_and_nothing_else() {
        let (_, written) = metadata(
            &ex("speed"),
            "Property",
            &json!({ngsi("hasValue"): [{"@value": 1}], ngsi("unitCode"): [{"@value": "KMH"}], ngsi("hasValueType"): [{"@id": ex("Speed")}]}),
        );
        let stored = HistoryMembers::unit_alone(Some(&written));
        assert!(stored.unit_code.is_some());
        pretty_assertions::assert_eq!(stored.value_type_key, None);
    }
}
