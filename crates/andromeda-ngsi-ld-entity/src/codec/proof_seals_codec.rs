//! The JSON object a store keeps the data-integrity seals of one instance in,
//! both ways (clause 4.5.2.2).
//!
//! The object holds the `entityIdSealed` and `entityTypeSealed` members exactly
//! as the instance object of the stored document holds them, each an array of
//! expanded values, so a seal reads back as the value it was written as.

use crate::{
    codec::{
        decode_context::DecodeContext,
        element::ElementsSeed,
        instance_encode::write_seals,
        member_decode::{malformed, seal_values},
        object_key::{ObjectKey, attribute_key},
    },
    member_role::{MemberRole, ProofSeal},
    member_shape::MemberShape,
    proof_seals::ProofSeals,
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde::{
    Deserializer,
    Serialize,
    Serializer,
    de::{DeserializeSeed, MapAccess, Visitor},
    ser::SerializeMap,
};
use std::fmt;

// Hand-written: the seals are written as the expanded instance members the
// codec writes, which no derive spells.
impl Serialize for ProofSeals {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(usize::from(self.entity_id().is_some()) + usize::from(self.entity_type().is_some())))?;
        write_seals(&mut map, self)?;
        map.end()
    }
}

impl ProofSeals {
    /// Read the seals of an instance of `attribute` from the JSON object a
    /// store keeps them in.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a text that is no such object: a
    /// member other than the two seals, a value that does not read as an
    /// expanded value, the NGSI-LD Null anywhere.
    pub fn from_stored(attribute: &AttributeName, json: &str) -> Result<ProofSeals, EntityRefusal> {
        let context = DecodeContext::default();
        let mut deserializer = serde_json::Deserializer::from_str(json);
        SealsSeed { context: &context, attribute }
            .deserialize(&mut deserializer)
            .map_err(|error| context.into_refusal(&error))
    }
}

/// Reads the stored seals of one instance.
struct SealsSeed<'c> {
    /// Where a refusal is recorded.
    context: &'c DecodeContext,
    /// The attribute the instance belongs to.
    attribute: &'c AttributeName,
}

impl<'de> DeserializeSeed<'de> for SealsSeed<'_> {
    type Value = ProofSeals;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<ProofSeals, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for SealsSeed<'_> {
    type Value = ProofSeals;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the data-integrity seals of an instance")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ProofSeals, A::Error> {
        let refuse = |refusal: EntityRefusal| self.context.refuse::<A::Error>(refusal);
        let elements = ElementsSeed {
            context: self.context,
            owner: Some(self.attribute),
        };
        let (mut entity_id, mut entity_type) = (None, None);
        while let Some(key) = map.next_key::<ObjectKey>()? {
            let seal = match key {
                ObjectKey::Name(name) => match MemberRole::of(attribute_key(&name).map_err(refuse)?.as_str()) {
                    MemberRole::Seal(seal) => seal,
                    MemberRole::Carrier(_)
                    | MemberRole::DatasetId
                    | MemberRole::InstanceId
                    | MemberRole::ObservedAt
                    | MemberRole::ExpiresAt
                    | MemberRole::DeletedAt
                    | MemberRole::CreatedAt
                    | MemberRole::ModifiedAt
                    | MemberRole::UnitCode
                    | MemberRole::ValueType
                    | MemberRole::ObjectType
                    | MemberRole::OutputOnly
                    | MemberRole::Scope
                    | MemberRole::Attribute => return Err(refuse(malformed(Some(self.attribute), MemberShape::Attribute))),
                },
                ObjectKey::Value
                | ObjectKey::Type
                | ObjectKey::Id
                | ObjectKey::Language
                | ObjectKey::Direction
                | ObjectKey::List
                | ObjectKey::OtherKeyword
                | ObjectKey::InvalidName(_) => return Err(refuse(malformed(Some(self.attribute), MemberShape::Attribute))),
            };
            let values = seal_values(map.next_value_seed(elements)?, self.attribute).map_err(refuse)?;
            match seal {
                ProofSeal::EntityId => entity_id = Some(values),
                ProofSeal::EntityType => entity_type = Some(values),
            }
        }
        Ok(ProofSeals::new(entity_id, entity_type))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        expanded_value::ExpandedValue,
        literal::{Literal, LiteralValue},
        proof_seals::ProofSeals,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_vocabulary::expanded::properties;

    /// The `ngsildproof` attribute.
    fn proof() -> AttributeName {
        properties::NGSILDPROOF.as_str().parse().expect("the proof attribute")
    }

    /// A plain string value.
    fn text(value: &str) -> ExpandedValue {
        ExpandedValue::Literal(Literal::plain(LiteralValue::String(value.into())))
    }

    #[test]
    fn both_seals_round_trip_through_their_stored_object() {
        let seals = ProofSeals::new(Some(vec![text("urn:ngsi-ld:Building:1")]), Some(vec![text("Building"), text("Shop")]));
        let stored = serde_json::to_string(&seals).expect("the seals encode");
        pretty_assertions::assert_eq!(ProofSeals::from_stored(&proof(), &stored).expect("the stored seals"), seals);
    }

    #[test]
    fn one_seal_round_trips_alone() {
        let seals = ProofSeals::new(None, Some(vec![text("Building")]));
        let stored = serde_json::to_string(&seals).expect("the seals encode");
        pretty_assertions::assert_eq!(ProofSeals::from_stored(&proof(), &stored).expect("the stored seals"), seals);
    }

    #[test]
    fn a_member_other_than_a_seal_is_refused() {
        let stored = r#"{"https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": "x"}]}"#;
        assert!(ProofSeals::from_stored(&proof(), stored).is_err());
    }
}
