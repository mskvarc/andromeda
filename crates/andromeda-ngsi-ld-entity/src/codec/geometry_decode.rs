//! Reading the `value` of a `GeoProperty` instance (clause 4.7).
//!
//! The value is one element: the `GeoJSON` vocabulary node expansion produces
//! from a geometry object, the clause 4.7.2 string literal, or the NGSI-LD
//! Null of a deletion. Clause 4.7.1 admits nothing else: a `GeometryCollection`,
//! a JSON literal, or any other value is refused. The node's `coordinates` are
//! read straight into positions when its `@type` came first, as it does in
//! every document the broker or PostgreSQL writes; otherwise they are kept as
//! expanded values and read once the type is known. Its `bbox` (IETF RFC 7946
//! section 5, the core context's `geojson:bbox` `@list` term) and its foreign
//! members (section 6.1, any other expanded name) are read beside them; the
//! NGSI-LD Null is refused inside either, as entity state never holds it.

use crate::{
    codec::{
        coordinate_list::GeometryReading,
        coordinates_decode::CoordinatesSeed,
        decode_context::DecodeContext,
        element::{ElementsSeed, TypeEntries, values_without_null},
        geojson_members_codec::MembersRead,
        object_key::ObjectKey,
        raw_scalar::RawScalar,
        value_encode::ValuesOut,
    },
    expanded_value::ExpandedValue,
    geo_value::{EncodedGeometry, GeoValue},
    geometry_value::GeometryValue,
    member_shape::MemberShape,
    ngsi_ld_null::is_ngsi_ld_null,
    refusal::EntityRefusal,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geojson::GeometryError, geometry_type::GeometryType};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::{expanded::properties, prefix};
use serde::{
    Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::{fmt, str::FromStr};

/// The `GeoJSON` `type` token of a `GeometryCollection`, which clause 4.7.1
/// names only to exclude.
const GEOMETRY_COLLECTION_TOKEN: &str = "GeometryCollection";

/// What a `GeoProperty`'s `value` holds.
#[derive(Debug)]
pub(crate) enum GeoDecoded {
    /// A geometry.
    Value(GeoValue),
    /// The NGSI-LD Null: the deletion encoding of clause 4.5.0.
    Null,
}

/// Reads the `value` array of a `GeoProperty` instance.
#[derive(Clone, Copy)]
pub(crate) struct GeoValueSeed<'c> {
    /// Where a refusal is recorded.
    pub(crate) context: &'c DecodeContext,
    /// The `GeoProperty`.
    pub(crate) attribute: &'c AttributeName,
}

impl GeoValueSeed<'_> {
    /// The refusal of a `value` that is not one geometry element.
    fn malformed(self, member: MemberShape) -> EntityRefusal {
        EntityRefusal::Malformed {
            attribute: Some(self.attribute.clone()),
            member,
        }
    }

    /// The refusal of a geometry fault.
    fn geometry(self, source: GeometryError) -> EntityRefusal {
        EntityRefusal::GeoProperty {
            attribute: self.attribute.clone(),
            source,
        }
    }
}

impl<'de> DeserializeSeed<'de> for GeoValueSeed<'_> {
    type Value = GeoDecoded;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<GeoDecoded, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for GeoValueSeed<'_> {
    type Value = GeoDecoded;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the value of a GeoProperty")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<GeoDecoded, A::Error> {
        let Some(element) = seq.next_element_seed(GeoElementSeed { seed: self })? else {
            return Err(self.context.refuse(self.malformed(MemberShape::Value)));
        };
        if seq.next_element::<IgnoredAny>()?.is_some() {
            return Err(self.context.refuse(self.malformed(MemberShape::Value)));
        }
        Ok(element)
    }
}

/// Reads the one element of a `GeoProperty`'s `value`.
#[derive(Clone, Copy)]
struct GeoElementSeed<'c> {
    /// The `value` being read.
    seed: GeoValueSeed<'c>,
}

impl<'de> DeserializeSeed<'de> for GeoElementSeed<'_> {
    type Value = GeoDecoded;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<GeoDecoded, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for GeoElementSeed<'_> {
    type Value = GeoDecoded;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a GeoJSON node or a string geometry")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<GeoDecoded, A::Error> {
        let seed = self.seed;
        let refuse = |refusal: EntityRefusal| seed.context.refuse::<A::Error>(refusal);
        let mut parts = GeoParts::default();
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::Value => parts.value = Some(map.next_value::<RawScalar>()?),
                ObjectKey::Type => match map.next_value::<TypeEntries>()? {
                    TypeEntries::Array(types) => parts.geometry_type = Some(geometry_type(&types).map_err(|source| refuse(seed.geometry(source)))?),
                    TypeEntries::Single(_) => return Err(refuse(seed.malformed(MemberShape::Geometry))),
                },
                ObjectKey::Name(name) if name.as_str() == properties::COORDINATES.as_str() => {
                    if let Some(geometry_type) = parts.geometry_type {
                        let reading = GeometryReading {
                            context: seed.context,
                            attribute: seed.attribute,
                            geometry_type,
                        };
                        parts.form = Some(map.next_value_seed(CoordinatesSeed { reading: &reading })?);
                    } else {
                        let elements = map.next_value_seed(ElementsSeed {
                            context: seed.context,
                            owner: Some(seed.attribute),
                        })?;
                        parts.pending = Some(values_without_null(elements, Some(seed.attribute)).map_err(refuse)?);
                    }
                }
                ObjectKey::Name(name) => parts.members.read(seed.context, seed.attribute, name, &mut map)?,
                ObjectKey::Id | ObjectKey::Language | ObjectKey::Direction | ObjectKey::List | ObjectKey::OtherKeyword | ObjectKey::InvalidName(_) => {
                    return Err(refuse(seed.malformed(MemberShape::Geometry)));
                }
            }
        }
        parts.decide(seed).map_err(refuse)
    }
}

/// The members of a geometry element, gathered before it is decided.
#[derive(Default)]
struct GeoParts {
    /// `@value` of the string form or of the Null.
    value: Option<RawScalar>,
    /// The geometry type the node's `@type` names.
    geometry_type: Option<GeometryType>,
    /// The coordinates, read after the type.
    form: Option<GeometryForm>,
    /// The coordinates, read before the type was known.
    pending: Option<Vec<ExpandedValue<Fragment>>>,
    /// The `bbox` and the foreign members.
    members: MembersRead,
}

impl GeoParts {
    /// Decide what the gathered members make.
    fn decide(self, seed: GeoValueSeed<'_>) -> Result<GeoDecoded, EntityRefusal> {
        let GeoParts {
            value,
            geometry_type,
            form,
            pending,
            members,
        } = self;
        let members = members.finish();
        // A value object carries no member beside its `@value`.
        let beside_value = members.is_some();
        let geometry = |form| GeoDecoded::Value(GeoValue::Geometry(GeometryValue::with_members(form, members)));
        match (value, geometry_type, form, pending) {
            (Some(_), None, None, None) if beside_value => Err(seed.malformed(MemberShape::Geometry)),
            (Some(RawScalar::String(text)), None, None, None) if is_ngsi_ld_null(&text) => Ok(GeoDecoded::Null),
            (Some(RawScalar::String(text)), None, None, None) => EncodedGeometry::parse(text)
                .map(|encoded| GeoDecoded::Value(GeoValue::Encoded(encoded)))
                .map_err(|source| seed.geometry(source)),
            (Some(RawScalar::Number(_) | RawScalar::Boolean(_) | RawScalar::Json(_)), None, None, None) => Err(seed.geometry(GeometryError::NotAnObject)),
            (None, Some(_), Some(form), None) => Ok(geometry(form)),
            (None, Some(geometry_type), None, Some(pending)) => read_pending(seed, geometry_type, &pending).map(geometry),
            (None, Some(_), None, None) => Err(seed.geometry(GeometryError::MissingCoordinates)),
            (None, None, None, _) => Err(seed.geometry(GeometryError::MissingType)),
            (Some(_), _, _, _) | (None, None, Some(_), _) | (None, Some(_), Some(_), Some(_)) => Err(seed.malformed(MemberShape::Geometry)),
        }
    }
}

/// The geometry type a node's `@type` names, refusing a `GeometryCollection`
/// (clause 4.7.1) and any type RFC 7946 does not define.
fn geometry_type(types: &[String]) -> Result<GeometryType, GeometryError> {
    let token = types
        .first()
        .and_then(|iri| iri.strip_prefix(prefix::GEOJSON.as_str()))
        .ok_or(GeometryError::MissingType)?;
    if token == GEOMETRY_COLLECTION_TOKEN {
        return Err(GeometryError::GeometryCollectionUnsupported);
    }
    GeometryType::from_str(token).map_err(GeometryError::UnknownType)
}

/// Read coordinates that arrived before the node's type: they are written back
/// in their expanded form and read again with the type known, so one reader
/// decides every geometry.
fn read_pending(seed: GeoValueSeed<'_>, geometry_type: GeometryType, pending: &[ExpandedValue<Fragment>]) -> Result<GeometryForm, EntityRefusal> {
    let bytes = serde_json::to_vec(&ValuesOut(pending)).map_err(|_| seed.malformed(MemberShape::Geometry))?;
    let context = DecodeContext::default();
    let reading = GeometryReading {
        context: &context,
        attribute: seed.attribute,
        geometry_type,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    CoordinatesSeed { reading: &reading }
        .deserialize(&mut deserializer)
        .map_err(|error| context.into_refusal(&error))
}

/// Read the expanded values of a written `value` as a `GeoProperty` geometry:
/// what a fragment that declared no family carries when a write applies it to
/// a stored `GeoProperty`.
pub(crate) fn geo_value_of(attribute: &AttributeName, values: &[ExpandedValue<Fragment>]) -> Result<GeoDecoded, EntityRefusal> {
    let bytes = serde_json::to_vec(&ValuesOut(values)).map_err(|_| EntityRefusal::Malformed {
        attribute: Some(attribute.clone()),
        member: MemberShape::Value,
    })?;
    let context = DecodeContext::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    GeoValueSeed { context: &context, attribute }
        .deserialize(&mut deserializer)
        .map_err(|error| context.into_refusal(&error))
}
