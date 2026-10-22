//! The `type` token of a `GeoJSON` geometry object (IETF RFC 7946 sections 1.4
//! and 3.1), and what ETSI GS CIM 009 v1.9.1 admits of it.
//!
//! RFC 7946 names seven geometry types: the six whose positions a
//! `coordinates` member holds, which [`GeometryType`] names, and the
//! `GeometryCollection` of section 3.1.8, which holds `geometries` instead.
//! Clause 4.7.1 admits every one of them as a `GeoProperty` value except the
//! collection. Every reader of a geometry object, compact or expanded, resolves
//! its token here, so the token is spelled once and the collection is refused
//! with one reason.

use crate::{geojson::GeometryError, geometry_member::GeometryMember, geometry_type::GeometryType};
use std::str::FromStr;

/// The `type` token of a `GeometryCollection`.
const GEOMETRY_COLLECTION: &str = "GeometryCollection";

/// A geometry type RFC 7946 defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryObjectType {
    /// A geometry whose positions `coordinates` holds.
    Geometry(GeometryType),
    /// A `GeometryCollection`.
    Collection,
}

impl GeometryObjectType {
    /// The member that holds what a geometry of this type is made of.
    #[must_use]
    pub const fn holding_member(self) -> GeometryMember {
        match self {
            GeometryObjectType::Geometry(_) => GeometryMember::Coordinates,
            GeometryObjectType::Collection => GeometryMember::Geometries,
        }
    }

    /// The geometry type a `GeoProperty` value of this type has.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::GeometryCollectionUnsupported`] for a
    /// `GeometryCollection`, which clause 4.7.1 excludes.
    pub const fn geo_property_type(self) -> Result<GeometryType, GeometryError> {
        match self {
            GeometryObjectType::Geometry(geometry_type) => Ok(geometry_type),
            GeometryObjectType::Collection => Err(GeometryError::GeometryCollectionUnsupported),
        }
    }
}

// Hand-written: the token is either the collection or one of the six types
// `GeometryType` already parses, which no derive over this enum expresses.
impl FromStr for GeometryObjectType {
    type Err = strum::ParseError;

    fn from_str(token: &str) -> Result<GeometryObjectType, strum::ParseError> {
        if token == GEOMETRY_COLLECTION {
            return Ok(GeometryObjectType::Collection);
        }
        GeometryType::from_str(token).map(GeometryObjectType::Geometry)
    }
}

#[cfg(test)]
mod tests {
    use crate::{geojson::GeometryError, geometry_member::GeometryMember, geometry_object_type::GeometryObjectType, geometry_type::GeometryType};

    #[test]
    fn every_rfc_7946_geometry_type_is_read_from_its_token() {
        for (token, read) in [
            ("Point", GeometryObjectType::Geometry(GeometryType::Point)),
            ("MultiPoint", GeometryObjectType::Geometry(GeometryType::MultiPoint)),
            ("LineString", GeometryObjectType::Geometry(GeometryType::LineString)),
            ("MultiLineString", GeometryObjectType::Geometry(GeometryType::MultiLineString)),
            ("Polygon", GeometryObjectType::Geometry(GeometryType::Polygon)),
            ("MultiPolygon", GeometryObjectType::Geometry(GeometryType::MultiPolygon)),
            ("GeometryCollection", GeometryObjectType::Collection),
        ] {
            pretty_assertions::assert_eq!(token.parse::<GeometryObjectType>(), Ok(read), "{token}");
        }
    }

    /// RFC 7946 section 1.4 spells the tokens case-sensitively, and the
    /// `GeoJSON` object types that are no geometry name none.
    #[test]
    fn a_token_rfc_7946_gives_no_geometry_is_refused() {
        for token in ["point", "geometrycollection", "Feature", "FeatureCollection", ""] {
            assert!(token.parse::<GeometryObjectType>().is_err(), "{token}");
        }
    }

    #[test]
    fn a_collection_holds_geometries_and_every_other_type_coordinates() {
        pretty_assertions::assert_eq!(GeometryObjectType::Collection.holding_member(), GeometryMember::Geometries);
        pretty_assertions::assert_eq!(
            GeometryObjectType::Geometry(GeometryType::Polygon).holding_member(),
            GeometryMember::Coordinates
        );
    }

    /// Clause 4.7.1 admits every RFC 7946 geometry but the collection.
    #[test]
    fn only_the_collection_is_no_geo_property_type() {
        assert!(matches!(
            GeometryObjectType::Collection.geo_property_type(),
            Err(GeometryError::GeometryCollectionUnsupported)
        ));
        assert!(matches!(
            GeometryObjectType::Geometry(GeometryType::Point).geo_property_type(),
            Ok(GeometryType::Point)
        ));
    }
}
