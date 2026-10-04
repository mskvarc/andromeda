//! The `bbox` and the foreign members of a geometry (IETF RFC 7946 sections 5
//! and 6.1) as the stored JSON holds them: the members of the expanded
//! `GeoJSON` node beside its `@type` and `coordinates`.
//!
//! The stored document writes them inside the geometry node; a history row and
//! a durable intent, which keep a geometry's positions apart, keep them as an
//! object of their own in the same form. One reading serves both.

use crate::{
    codec::{
        decode_context::DecodeContext,
        element::{ElementsSeed, values_without_null},
        geometry_encode::BoundingBoxOut,
        object_key::ObjectKey,
        value_encode::ValuesOut,
    },
    conversion::stored_value::into_stored_values,
    expanded_value::ExpandedValue,
    expanded_value_name::ExpandedValueName,
    geometry_value::ExpandedGeoJsonMembers,
    literal::LiteralValue,
    member_shape::MemberShape,
    refusal::EntityRefusal,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_geometry::{
    bounding_box::{BoundingBox, BoundingBoxError},
    geojson::GeometryError,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use indexmap::IndexMap;
use serde::{
    Deserializer,
    Serialize,
    Serializer,
    de::{DeserializeSeed, MapAccess, Visitor},
    ser::SerializeMap,
};
use std::fmt;

/// The members of one geometry node beside its positions, gathered as they
/// are read.
#[derive(Default)]
pub(crate) struct MembersRead {
    /// The `bbox`, once read.
    bbox: Option<BoundingBox>,
    /// The foreign members, by expanded name.
    foreign: IndexMap<ExpandedValueName, Vec<ExpandedValue>>,
}

impl MembersRead {
    /// Read the value of the member `name` of a geometry node of `attribute`:
    /// the `bbox`, or a foreign member.
    pub(crate) fn read<'de, A: MapAccess<'de>>(
        &mut self,
        context: &DecodeContext,
        attribute: &AttributeName,
        name: ExpandedValueName,
        map: &mut A,
    ) -> Result<(), A::Error> {
        let elements = map.next_value_seed(ElementsSeed {
            context,
            owner: Some(attribute),
        })?;
        let values = values_without_null(elements, Some(attribute)).map_err(|refusal| context.refuse(refusal))?;
        if name.as_str() == properties::BBOX.as_str() {
            if self.bbox.is_some() {
                return Err(context.refuse(EntityRefusal::Malformed {
                    attribute: Some(attribute.clone()),
                    member: MemberShape::Geometry,
                }));
            }
            let bbox = bounding_box(&values).map_err(|source| {
                context.refuse(EntityRefusal::GeoProperty {
                    attribute: attribute.clone(),
                    source,
                })
            })?;
            self.bbox = Some(bbox);
        } else {
            // Entity state holds the NGSI-LD Null nowhere (clause 4.5.0).
            let stored = into_stored_values(values).ok_or_else(|| {
                context.refuse(EntityRefusal::NullInState {
                    attribute: Some(attribute.clone()),
                })
            })?;
            self.foreign.insert(name, stored);
        }
        Ok(())
    }

    /// The members read, or `None` when there was none.
    pub(crate) fn finish(self) -> Option<ExpandedGeoJsonMembers> {
        ExpandedGeoJsonMembers::non_empty(self.bbox, self.foreign)
    }
}

/// The bounding box of an expanded `bbox` member: one list of numbers.
fn bounding_box(values: &[ExpandedValue<Fragment>]) -> Result<BoundingBox, GeometryError> {
    let [ExpandedValue::List(items)] = values else {
        return Err(GeometryError::InvalidBbox(BoundingBoxError::NotNumbers));
    };
    let ordinates = items
        .iter()
        .map(|item| match item {
            ExpandedValue::Literal(literal) => match literal.value() {
                LiteralValue::Number(number) => number.to_serde_number().as_f64().filter(|ordinate| ordinate.is_finite()),
                LiteralValue::String(_) | LiteralValue::Boolean(_) => None,
            },
            ExpandedValue::Json(_) | ExpandedValue::Node(_) | ExpandedValue::List(_) => None,
        })
        .collect::<Option<Vec<f64>>>()
        .ok_or(GeometryError::InvalidBbox(BoundingBoxError::NotNumbers))?;
    Ok(BoundingBox::try_from_ordinates(&ordinates)?)
}

/// Write the members as entries of the geometry node `map` is writing.
pub(crate) fn write_members<M: SerializeMap>(map: &mut M, members: &ExpandedGeoJsonMembers) -> Result<(), M::Error> {
    if let Some(bbox) = members.bbox() {
        map.serialize_entry(properties::BBOX.as_str(), &[BoundingBoxOut(bbox)])?;
    }
    for (name, values) in members.foreign() {
        map.serialize_entry(name.as_str(), &ValuesOut(values))?;
    }
    Ok(())
}

// Hand-written: the members are written as the expanded node members the
// codec writes, which no derive spells. This is the JSON object a history row
// and a durable intent store them as.
impl Serialize for ExpandedGeoJsonMembers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(usize::from(self.bbox().is_some()) + self.foreign().len()))?;
        write_members(&mut map, self)?;
        map.end()
    }
}

impl ExpandedGeoJsonMembers {
    /// Read the members of a geometry of `attribute` from the JSON object a
    /// history row or a durable intent stores them as.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a text that is no such object: a
    /// member that is no expanded name, a `bbox` RFC 7946 section 5 refuses,
    /// the NGSI-LD Null anywhere.
    pub fn from_stored(attribute: &AttributeName, json: &str) -> Result<Option<ExpandedGeoJsonMembers>, EntityRefusal> {
        let context = DecodeContext::default();
        let mut deserializer = serde_json::Deserializer::from_str(json);
        MembersSeed { context: &context, attribute }
            .deserialize(&mut deserializer)
            .map_err(|error| context.into_refusal(&error))
    }
}

/// Reads the stored members of one geometry.
struct MembersSeed<'c> {
    /// Where a refusal is recorded.
    context: &'c DecodeContext,
    /// The attribute the geometry belongs to.
    attribute: &'c AttributeName,
}

impl<'de> DeserializeSeed<'de> for MembersSeed<'_> {
    type Value = Option<ExpandedGeoJsonMembers>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Option<ExpandedGeoJsonMembers>, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for MembersSeed<'_> {
    type Value = Option<ExpandedGeoJsonMembers>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the members of a geometry node")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Option<ExpandedGeoJsonMembers>, A::Error> {
        let mut members = MembersRead::default();
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::Name(name) if name.as_str() != properties::COORDINATES.as_str() => members.read(self.context, self.attribute, name, &mut map)?,
                ObjectKey::Name(_)
                | ObjectKey::Value
                | ObjectKey::Type
                | ObjectKey::Id
                | ObjectKey::Language
                | ObjectKey::Direction
                | ObjectKey::List
                | ObjectKey::OtherKeyword
                | ObjectKey::InvalidName(_) => {
                    return Err(self.context.refuse(EntityRefusal::Malformed {
                        attribute: Some(self.attribute.clone()),
                        member: MemberShape::Geometry,
                    }));
                }
            }
        }
        Ok(members.finish())
    }
}

#[cfg(test)]
mod tests {
    use crate::{geometry_value::ExpandedGeoJsonMembers, refusal::EntityRefusal};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    fn attribute() -> AttributeName {
        "https://uri.etsi.org/ngsi-ld/location".parse().expect("an attribute name")
    }

    /// The stored members read back and write out as the same JSON.
    #[test]
    fn stored_members_round_trip() {
        let stored = r#"{"https://purl.org/geojson/vocab#bbox":[{"@list":[{"@value":1},{"@value":2},{"@value":3},{"@value":4}]}],"https://ex.org/plotName":[{"@value":"plot 7"}]}"#;
        let members = ExpandedGeoJsonMembers::from_stored(&attribute(), stored).expect("readable").expect("members");
        pretty_assertions::assert_eq!(serde_json::to_string(&members).expect("writable"), stored);
    }

    /// An empty object is no member at all.
    #[test]
    fn an_empty_object_holds_no_member() {
        assert!(ExpandedGeoJsonMembers::from_stored(&attribute(), "{}").expect("readable").is_none());
    }

    /// The positions are no member: `coordinates` and `@type` are refused.
    #[test]
    fn coordinates_and_a_type_are_refused() {
        for stored in [
            r#"{"https://purl.org/geojson/vocab#coordinates":[]}"#,
            r#"{"@type":["https://purl.org/geojson/vocab#Point"]}"#,
        ] {
            assert!(
                matches!(ExpandedGeoJsonMembers::from_stored(&attribute(), stored), Err(EntityRefusal::Malformed { .. })),
                "{stored}"
            );
        }
    }
}
