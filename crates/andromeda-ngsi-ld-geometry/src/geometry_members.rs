//! What the members of a compact `GeoJSON` geometry object say, and the
//! geometry they spell.
//!
//! A geometry object is read member by member, from a parsed tree or straight
//! from a deserializer, into [`GeometryMembers`]; only once every member is
//! known is the geometry built, because the `type` that decides how
//! `coordinates` nest may come after them. Members IETF RFC 7946 section 6.1
//! calls foreign are not kept, as the geometry carries none of them; whether
//! one of them is a JSON-LD keyword is.

use crate::{coordinate_tape::CoordinateTape, form::GeometryForm, geojson::GeometryError, geometry_type::GeometryType};
use serde_json::{Map, Value};
use std::str::FromStr;

/// The `GeoJSON` `type` token of a `GeometryCollection`, which ETSI GS CIM 009
/// v1.9.1 clause 4.7.1 excludes from a `GeoProperty` value.
const GEOMETRY_COLLECTION_TOKEN: &str = "GeometryCollection";

/// What the `type` member of a geometry object says.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum TypeMember {
    /// No `type` member, or one that is not a string.
    #[default]
    Missing,
    /// A `GeometryCollection`.
    Collection,
    /// A geometry type RFC 7946 section 1.4 defines.
    Known(GeometryType),
    /// A string naming no geometry type RFC 7946 defines.
    Unknown(strum::ParseError),
}

impl TypeMember {
    /// What the string `token` says as a `type` member.
    pub(crate) fn of(token: &str) -> TypeMember {
        if token == GEOMETRY_COLLECTION_TOKEN {
            return TypeMember::Collection;
        }
        GeometryType::from_str(token).map_or_else(TypeMember::Unknown, TypeMember::Known)
    }
}

/// Whether a geometry object carries a `geometries` array.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GeometriesMember {
    /// No `geometries` member, or one that is not an array.
    #[default]
    Absent,
    /// A `geometries` array.
    Array,
}

/// What the `bbox` member of a geometry object says (RFC 7946 section 5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BboxMember {
    /// No `bbox` member, or an array of numbers. The box itself is not
    /// carried.
    #[default]
    Admitted,
    /// A `bbox` that is not an array of numbers.
    Invalid,
}

/// Whether a geometry object carries a member named by a JSON-LD keyword.
///
/// A JSON-LD keyword is no `GeoJSON` member at all but JSON-LD a processor
/// would act on, such as an `@context`, so a reader that must not let one pass
/// unseen asks for it; the geometry itself never carries it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JsonLdKeywords {
    /// No member name starts with `@`.
    #[default]
    Absent,
    /// At least one member name starts with `@`.
    Present,
}

impl JsonLdKeywords {
    /// What the member `name` says about the keywords of its object.
    pub(crate) fn of(name: &str) -> JsonLdKeywords {
        if name.starts_with('@') {
            JsonLdKeywords::Present
        } else {
            JsonLdKeywords::Absent
        }
    }

    /// The keywords of an object holding the members of both.
    pub(crate) const fn and(self, other: JsonLdKeywords) -> JsonLdKeywords {
        match (self, other) {
            (JsonLdKeywords::Absent, JsonLdKeywords::Absent) => JsonLdKeywords::Absent,
            (JsonLdKeywords::Present, JsonLdKeywords::Absent | JsonLdKeywords::Present) | (JsonLdKeywords::Absent, JsonLdKeywords::Present) => {
                JsonLdKeywords::Present
            }
        }
    }
}

/// The members of one geometry object that decide the geometry it is.
///
/// A member written twice counts as written last, as a parsed tree keeps it.
#[derive(Debug, Default)]
pub(crate) struct GeometryMembers {
    /// What the `type` member says.
    pub(crate) geometry_type: TypeMember,
    /// Whether a `geometries` array is present.
    pub(crate) geometries: GeometriesMember,
    /// What the `bbox` member says.
    pub(crate) bbox: BboxMember,
    /// The `coordinates` member, when present.
    pub(crate) coordinates: Option<CoordinateTape>,
    /// Whether any member is named by a JSON-LD keyword.
    pub(crate) json_ld_keywords: JsonLdKeywords,
}

impl GeometryMembers {
    /// The members of a geometry object already parsed into a tree.
    pub(crate) fn of_object(object: &Map<String, Value>) -> GeometryMembers {
        GeometryMembers {
            geometry_type: object.get("type").and_then(Value::as_str).map_or(TypeMember::Missing, TypeMember::of),
            geometries: match object.get("geometries") {
                Some(Value::Array(_)) => GeometriesMember::Array,
                Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) | None => GeometriesMember::Absent,
            },
            bbox: match object.get("bbox") {
                None => BboxMember::Admitted,
                Some(Value::Array(members)) if members.iter().all(Value::is_number) => BboxMember::Admitted,
                Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_)) => BboxMember::Invalid,
            },
            coordinates: object.get("coordinates").map(CoordinateTape::of_value),
            json_ld_keywords: object
                .keys()
                .fold(JsonLdKeywords::Absent, |keywords, name| keywords.and(JsonLdKeywords::of(name))),
        }
    }

    /// The geometry the members spell.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::MissingType`], [`GeometryError::UnknownType`],
    /// [`GeometryError::MissingCoordinates`],
    /// [`GeometryError::MissingGeometries`] or [`GeometryError::InvalidBbox`]
    /// for members that spell no `GeoJSON` geometry,
    /// [`GeometryError::GeometryCollectionUnsupported`] for a
    /// `GeometryCollection`, and the refusals of the coordinates' nesting.
    pub(crate) fn read(&self) -> Result<GeometryForm, GeometryError> {
        let geometry_type = match self.geometry_type {
            TypeMember::Missing => return Err(GeometryError::MissingType),
            TypeMember::Collection => {
                return Err(match self.geometries {
                    GeometriesMember::Array => GeometryError::GeometryCollectionUnsupported,
                    GeometriesMember::Absent => GeometryError::MissingGeometries,
                });
            }
            TypeMember::Unknown(refusal) => return Err(GeometryError::UnknownType(refusal)),
            TypeMember::Known(geometry_type) => geometry_type,
        };
        if self.bbox == BboxMember::Invalid {
            return Err(GeometryError::InvalidBbox);
        }
        self.coordinates
            .as_ref()
            .ok_or(GeometryError::MissingCoordinates)
            .and_then(|coordinates| coordinates.read_as(geometry_type))
    }
}
