//! What the members of a compact `GeoJSON` geometry object say, and the
//! geometry they spell.
//!
//! A geometry object is read member by member, straight from a deserializer,
//! into [`GeometryMembers`]; only once every member is
//! known is the geometry built, because the `type` that decides how
//! `coordinates` nest may come after them. The `bbox` of IETF RFC 7946 section
//! 5 and the members section 6.1 calls foreign are kept beside the form, so the
//! geometry reads back as it was written (ETSI GS CIM 009 v1.9.1 clause
//! 4.6.4); a member named by a JSON-LD keyword is reported, never kept.

use crate::{
    bounding_box::BoundingBox,
    coordinate_tape::CoordinateTape,
    foreign_members::ForeignMembers,
    form::GeometryForm,
    geojson::GeometryError,
    geojson_members::GeoJsonMembers,
    geometry::Geometry,
    geometry_object_type::GeometryObjectType,
};

/// What the `type` member of a geometry object says.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum TypeMember {
    /// No `type` member, or one that is not a string.
    #[default]
    Missing,
    /// A geometry type RFC 7946 section 1.4 defines.
    Known(GeometryObjectType),
    /// A string naming no geometry type RFC 7946 defines.
    Unknown(strum::ParseError),
}

impl TypeMember {
    /// What the string `token` says as a `type` member.
    pub(crate) fn of(token: &str) -> TypeMember {
        token.parse().map_or_else(TypeMember::Unknown, TypeMember::Known)
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

/// Whether reading a geometry object keeps its foreign members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForeignMemberReading {
    /// They are kept, for a reader that builds the whole geometry.
    Kept,
    /// They are passed over, for a reader that wants the form alone.
    Skipped,
}

/// The members of one geometry object.
///
/// A member written twice counts as written last.
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
    /// The geometry the members spell, with its `bbox` and its foreign
    /// members.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::MissingType`], [`GeometryError::UnknownType`],
    /// [`GeometryError::MissingCoordinates`],
    /// [`GeometryError::MissingGeometries`] or [`GeometryError::InvalidBbox`]
    /// for members that spell no `GeoJSON` geometry,
    /// [`GeometryError::GeometryCollectionUnsupported`] for a
    /// `GeometryCollection`, and the refusals of the coordinates' nesting.
    pub(crate) fn into_geometry(self) -> Result<Geometry, GeometryError> {
        let (form, bbox) = self.read_with_box()?;
        Ok(Geometry::with_members(form, GeoJsonMembers::non_empty(bbox, self.foreign)))
    }

    /// The form and the bounding box the members spell.
    fn read_with_box(&self) -> Result<(GeometryForm, Option<BoundingBox>), GeometryError> {
        let geometry_type = match (self.geometry_type, self.geometries) {
            (TypeMember::Missing, _) => return Err(GeometryError::MissingType),
            (TypeMember::Unknown(refusal), _) => return Err(GeometryError::UnknownType(refusal)),
            (TypeMember::Known(GeometryObjectType::Collection), GeometriesMember::Absent) => return Err(GeometryError::MissingGeometries),
            (TypeMember::Known(object_type), GeometriesMember::Absent | GeometriesMember::Array) => object_type.geo_property_type()?,
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
