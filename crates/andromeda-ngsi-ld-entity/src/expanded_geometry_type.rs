//! The geometry type an expanded `GeoJSON` node names (clause 4.7).
//!
//! Expansion under the core context types the node of a geometry with its IETF
//! RFC 7946 `type` token under the `GeoJSON` vocabulary, such as
//! `https://purl.org/geojson/vocab#Point`. A stored document and the expansion
//! of a request both name the type that way, and both read and write it here.

use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry_object_type::GeometryObjectType, geometry_type::GeometryType};
use andromeda_ngsi_ld_vocabulary::prefix;

/// The geometry type the `@type` of an expanded `GeoJSON` node names.
///
/// # Errors
///
/// Returns [`GeometryError::MissingType`] for a node naming no type under the
/// `GeoJSON` vocabulary, [`GeometryError::UnknownType`] for a type RFC 7946
/// does not define and [`GeometryError::GeometryCollectionUnsupported`] for a
/// collection, which clause 4.7.1 excludes.
pub fn expanded_geometry_type(types: &[String]) -> Result<GeometryType, GeometryError> {
    let token = types
        .first()
        .and_then(|iri| iri.strip_prefix(prefix::GEOJSON.as_str()))
        .ok_or(GeometryError::MissingType)?;
    token.parse::<GeometryObjectType>().map_err(GeometryError::UnknownType)?.geo_property_type()
}

/// The `@type` IRI of the expanded `GeoJSON` node of a geometry of
/// `geometry_type`.
#[must_use]
pub fn geometry_type_iri(geometry_type: GeometryType) -> String {
    format!("{}{geometry_type}", prefix::GEOJSON.as_str())
}

#[cfg(test)]
mod tests {
    use crate::expanded_geometry_type::{expanded_geometry_type, geometry_type_iri};
    use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry_type::GeometryType};

    fn types(iris: &[&str]) -> Vec<String> {
        iris.iter().map(|iri| (*iri).to_owned()).collect()
    }

    #[test]
    fn a_type_under_the_geojson_vocabulary_names_its_geometry() {
        assert!(matches!(
            expanded_geometry_type(&types(&["https://purl.org/geojson/vocab#MultiPolygon"])),
            Ok(GeometryType::MultiPolygon)
        ));
    }

    #[test]
    fn a_geometry_type_is_written_as_the_iri_it_is_read_from() {
        for geometry_type in [GeometryType::Point, GeometryType::LineString, GeometryType::Polygon] {
            let iri = geometry_type_iri(geometry_type);
            assert!(matches!(expanded_geometry_type(&[iri]), Ok(read) if read == geometry_type));
        }
        pretty_assertions::assert_eq!(geometry_type_iri(GeometryType::MultiPoint), "https://purl.org/geojson/vocab#MultiPoint");
    }

    #[test]
    fn a_node_naming_no_admitted_geometry_type_is_refused() {
        assert!(matches!(expanded_geometry_type(&[]), Err(GeometryError::MissingType)));
        assert!(matches!(
            expanded_geometry_type(&types(&["https://uri.etsi.org/ngsi-ld/default-context/Point"])),
            Err(GeometryError::MissingType)
        ));
        assert!(matches!(
            expanded_geometry_type(&types(&["https://purl.org/geojson/vocab#Feature"])),
            Err(GeometryError::UnknownType(_))
        ));
        assert!(matches!(
            expanded_geometry_type(&types(&["https://purl.org/geojson/vocab#GeometryCollection"])),
            Err(GeometryError::GeometryCollectionUnsupported)
        ));
    }
}
