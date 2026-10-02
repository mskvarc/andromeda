//! What the members of a compact `GeoJSON` geometry object say, and the
//! geometry they spell.
//!
//! A geometry object is read member by member, from a parsed tree or straight
//! from a deserializer, into [`GeometryMembers`]; only once every member is
//! known is the geometry built, because the `type` that decides how
//! `coordinates` nest may come after them. The `bbox` of IETF RFC 7946 section
//! 5 and the members section 6.1 calls foreign are kept beside the form, so the
//! geometry reads back as it was written (ETSI GS CIM 009 v1.9.1 clause
//! 4.6.4); a member named by a JSON-LD keyword is reported, never kept.

use crate::{
    bounding_box::BoundingBox,
    coordinate_tape::CoordinateTape,
    foreign_members::{ForeignMemberName, ForeignMembers},
    form::GeometryForm,
    geojson::GeometryError,
    geojson_members::GeoJsonMembers,
    geometry::Geometry,
    geometry_type::GeometryType,
};
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

/// Whether reading a parsed geometry object keeps its foreign members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForeignMemberReading {
    /// They are kept, for a reader that builds the whole geometry.
    Kept,
    /// They are passed over, for a reader that wants the form alone.
    Skipped,
}

/// The members of one geometry object.
///
/// A member written twice counts as written last, as a parsed tree keeps it.
#[derive(Debug, Default)]
pub(crate) struct GeometryMembers {
    /// What the `type` member says.
    pub(crate) geometry_type: TypeMember,
    /// Whether a `geometries` array is present.
    pub(crate) geometries: GeometriesMember,
    /// The `bbox` member, when present.
    pub(crate) bbox: Option<CoordinateTape>,
    /// The `coordinates` member, when present.
    pub(crate) coordinates: Option<CoordinateTape>,
    /// The foreign members, by name.
    pub(crate) foreign: ForeignMembers,
    /// Whether any member is named by a JSON-LD keyword.
    pub(crate) json_ld_keywords: JsonLdKeywords,
}

impl GeometryMembers {
    /// The members of a geometry object already parsed into a tree.
    pub(crate) fn of_object(object: &Map<String, Value>, reading: ForeignMemberReading) -> GeometryMembers {
        let mut foreign = ForeignMembers::default();
        if reading == ForeignMemberReading::Kept {
            for (name, value) in object {
                if let Ok(name) = ForeignMemberName::try_from(name.as_str()) {
                    foreign.insert(name, value.clone()); // clone: the geometry owns the member the borrowed tree holds
                }
            }
        }
        GeometryMembers {
            geometry_type: object.get("type").and_then(Value::as_str).map_or(TypeMember::Missing, TypeMember::of),
            geometries: match object.get("geometries") {
                Some(Value::Array(_)) => GeometriesMember::Array,
                Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) | None => GeometriesMember::Absent,
            },
            bbox: object.get("bbox").map(CoordinateTape::of_value),
            coordinates: object.get("coordinates").map(CoordinateTape::of_value),
            foreign,
            json_ld_keywords: object
                .keys()
                .fold(JsonLdKeywords::Absent, |keywords, name| keywords.and(JsonLdKeywords::of(name))),
        }
    }

    /// The geometry form the members spell, the `bbox` checked beside it.
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
        self.read_with_box().map(|(form, _)| form)
    }

    /// The geometry the members spell, with its `bbox` and its foreign
    /// members.
    ///
    /// # Errors
    ///
    /// As [`GeometryMembers::read`].
    pub(crate) fn into_geometry(self) -> Result<Geometry, GeometryError> {
        let (form, bbox) = self.read_with_box()?;
        Ok(Geometry::with_members(form, GeoJsonMembers::non_empty(bbox, self.foreign)))
    }

    /// The form and the bounding box the members spell.
    fn read_with_box(&self) -> Result<(GeometryForm, Option<BoundingBox>), GeometryError> {
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
        let bbox = self.bbox.as_ref().map(BoundingBox::of_tape).transpose()?;
        let form = self
            .coordinates
            .as_ref()
            .ok_or(GeometryError::MissingCoordinates)
            .and_then(|coordinates| coordinates.read_as(geometry_type))?;
        Ok((form, bbox))
    }
}
