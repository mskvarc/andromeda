//! The members an observed attribute instance carries beside its value.
//!
//! A history row keeps an instance's value, `datasetId`, `instanceId`,
//! `observedAt` and its clause 4.8 times in columns of their own. What remains
//! is the instance's metadata: its sub-attributes (clause 4.5.2) and the
//! companion members of its family (`unitCode`, `valueType`, `objectType`),
//! together with `expiresAt` (clause 4.22) and the data-integrity seals. A
//! `deletedAt` is no metadata: the broker generates it (Table 5.2.5-2), and a
//! history row recording a deletion holds it in a column of its own.
//!
//! The metadata is admitted once, when an observation is read: the
//! sub-attributes become committed attributes in the spelling storage keeps,
//! so every later reader (the store keying them, the renderer, the intent
//! wire) takes them as they are.

use andromeda_ngsi_ld_entity::{
    attributes::Attributes,
    document::{
        attribute_member::AttributeMember,
        instance_document::{InstanceDocument, InstanceDocumentParts},
        slot::Slot,
    },
    proof_seals::ProofSeals,
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReference, time::date_time::NgsiLdDateTime};
use cefact_units::UnitCode;

/// The metadata of one observed instance: never empty, and never holding the
/// value, the identity or the time axes the row keeps in columns.
///
/// Boxed because most observations carry none, and the row every observation
/// is held in should not pay for the members of those that do.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservationMetadata(Box<MetadataMembers>);

/// The metadata members of one observed instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetadataMembers {
    /// `expiresAt` (clause 4.22).
    pub expires_at: Option<NgsiLdDateTime>,
    /// `unitCode` (clause 4.5.2).
    pub unit_code: Option<UnitCode>,
    /// `valueType` (clause 4.5.2).
    pub value_type: Option<NgsiLdReference>,
    /// `objectType` (clause 4.5.3), in written order; empty when absent.
    pub object_types: Vec<NgsiLdReference>,
    /// The `entityIdSealed` and `entityTypeSealed` members (clause 4.5.2.2).
    pub proof_seals: ProofSeals,
    /// The sub-attributes, committed and in the spelling storage keeps, with
    /// no system time on any of them.
    pub sub_attributes: Attributes,
}

impl MetadataMembers {
    /// Whether no member is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.expires_at.is_none()
            && self.unit_code.is_none()
            && self.value_type.is_none()
            && self.object_types.is_empty()
            && self.proof_seals.is_empty()
            && self.sub_attributes.is_empty()
    }
}

impl ObservationMetadata {
    /// The metadata `members` make, `None` when they hold none.
    #[must_use]
    pub fn new(members: MetadataMembers) -> Option<ObservationMetadata> {
        (!members.is_empty()).then(|| ObservationMetadata(Box::new(members)))
    }

    /// Admit the metadata among the members of one written instance, `None`
    /// when the instance carries none.
    ///
    /// The value, the family, `datasetId`, `instanceId`, `observedAt`,
    /// `createdAt`, `modifiedAt` and `deletedAt` are not metadata and are not
    /// read. A sub-attribute of an observation is a Property or Relationship as
    /// clause 4.5.2 mandates, whatever the Temporal Evolution around it: its
    /// output-only members are ignored (Table 5.2.5-2), and every value takes
    /// the spelling storage keeps.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::NullInState`] when a companion member holds
    /// the NGSI-LD Null (clause 5.5.4 excepts the temporal evolution from the
    /// Null in a value position alone, a deleted instance of clause 4.5.7), and
    /// the model's refusal when a sub-attribute is no instance the committed
    /// model holds, the NGSI-LD Null in one included.
    pub fn admit(parts: InstanceDocumentParts) -> Result<Option<ObservationMetadata>, EntityRefusal> {
        let InstanceDocumentParts {
            expires_at,
            unit_code,
            value_type,
            object_types,
            proof_seals,
            mut sub_attributes,
            ..
        } = parts;
        let sub_attributes = if sub_attributes.is_empty() {
            Attributes::new()
        } else {
            sub_attributes.values_mut().for_each(AttributeMember::forget_output_only_members_throughout);
            let mut attributes = Attributes::try_from_members(sub_attributes)?;
            attributes.canonicalize_values();
            attributes
        };
        Ok(ObservationMetadata::new(MetadataMembers {
            expires_at: set(expires_at)?,
            unit_code: set(unit_code)?,
            value_type: set(value_type)?,
            object_types: set(object_types)?.unwrap_or_default(),
            proof_seals,
            sub_attributes,
        }))
    }

    /// The members.
    #[must_use]
    pub fn members(&self) -> &MetadataMembers {
        &self.0
    }

    /// Give the members up.
    #[must_use]
    pub fn into_members(self) -> MetadataMembers {
        *self.0
    }

    /// Give the metadata up as the members of an instance.
    #[must_use]
    pub fn into_parts(self) -> InstanceDocumentParts {
        InstanceDocument::from(self).into_parts()
    }
}

impl From<ObservationMetadata> for InstanceDocument {
    fn from(metadata: ObservationMetadata) -> InstanceDocument {
        let MetadataMembers {
            expires_at,
            unit_code,
            value_type,
            object_types,
            proof_seals,
            sub_attributes,
        } = metadata.into_members();
        InstanceDocument::builder()
            .expires_at(expires_at.map(Slot::Set))
            .unit_code(unit_code.map(Slot::Set))
            .value_type(value_type.map(Slot::Set))
            .object_types((!object_types.is_empty()).then_some(Slot::Set(object_types)))
            .proof_seals(proof_seals)
            .sub_attributes(sub_attributes.written_members())
            .build()
    }
}

/// The member a companion slot sets.
///
/// # Errors
///
/// Returns [`EntityRefusal::NullInState`] when the slot holds the NGSI-LD Null.
fn set<T>(slot: Option<Slot<T>>) -> Result<Option<T>, EntityRefusal> {
    match slot {
        Some(Slot::Set(member)) => Ok(Some(member)),
        Some(Slot::Remove) => Err(EntityRefusal::NullInState { attribute: None }),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::observation_metadata::ObservationMetadata;
    use andromeda_ngsi_ld_entity::document::{attribute_member::AttributeMember, instance_document::InstanceDocument};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::system_instant::SystemInstant};
    use indexmap::IndexMap;
    use serde_json::{Value, json};

    /// A Property sub-attribute carrying both system times beside `nested`.
    fn stamped(nested: &Value) -> Value {
        let mut instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
            "https://uri.etsi.org/ngsi-ld/createdAt": [{"@value": "1900-01-01T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}],
            "https://uri.etsi.org/ngsi-ld/modifiedAt": [{"@value": "1900-01-01T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]
        });
        if let (Some(members), Some(nested)) = (instance.as_object_mut(), nested.as_object()) {
            members.extend(nested.iter().map(|(name, value)| (name.clone(), value.clone()))); // clone: the fixture owns its members
        }
        json!([instance])
    }

    /// The system times of every sub-attribute instance, depth first.
    fn times(sub_attributes: &IndexMap<AttributeName, AttributeMember>) -> Vec<(Option<SystemInstant>, Option<SystemInstant>)> {
        sub_attributes
            .values()
            .flat_map(AttributeMember::instances)
            .flat_map(|instance| {
                let mut found = vec![(instance.created_at(), instance.modified_at())];
                found.extend(times(instance.sub_attributes()));
                found
            })
            .collect()
    }

    /// Clause 5.2.5: a client's `createdAt` and `modifiedAt` are ignored, so
    /// the metadata a history row stores keeps none on any sub-attribute.
    #[test]
    fn sub_attribute_system_times_are_not_kept_at_any_depth() {
        let speed: AttributeName = "https://example.org/speed".parse().expect("a valid name");
        let written = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}],
            "https://example.org/accuracy": stamped(&json!({"https://example.org/method": stamped(&json!({}))}))
        });
        let document = InstanceDocument::from_slice(&speed, &serde_json::to_vec(&written).expect("JSON")).expect("an expanded instance");

        let metadata = ObservationMetadata::admit(document.into_parts())
            .expect("the sub-attributes are committed")
            .expect("the sub-attributes are metadata");

        pretty_assertions::assert_eq!(times(&metadata.into_parts().sub_attributes), vec![(None, None), (None, None)]);
    }

    /// Clause 5.5.4 excepts the temporal evolution from the Null in a value
    /// position alone, so a companion member holding it is refused.
    #[test]
    fn a_companion_member_holding_the_null_is_refused() {
        let speed: AttributeName = "https://example.org/speed".parse().expect("a valid name");
        let written = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "urn:ngsi-ld:null"}]
        });
        let document = InstanceDocument::from_slice(&speed, &serde_json::to_vec(&written).expect("JSON")).expect("an expanded instance");
        assert!(ObservationMetadata::admit(document.into_parts()).is_err());
    }

    #[test]
    fn an_instance_carrying_no_metadata_member_has_no_metadata() {
        let speed: AttributeName = "https://example.org/speed".parse().expect("a valid name");
        let written = json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}]});
        let document = InstanceDocument::from_slice(&speed, &serde_json::to_vec(&written).expect("JSON")).expect("an expanded instance");
        assert!(ObservationMetadata::admit(document.into_parts()).expect("nothing to refuse").is_none());
    }
}
