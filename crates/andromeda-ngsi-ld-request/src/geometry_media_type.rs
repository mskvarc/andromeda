//! The media type a `geometryProperty` request parameter requires.

use crate::error::ParameterError;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Hold a request to the rule that `geometryProperty` selects the geometry of
/// a `GeoJSON` answer and so needs a `GeoJSON` `Accept`.
///
/// Clauses 5.7.1.4 (Retrieve Entity), 5.7.2.4 (Query Entities) and 5.14.4.4
/// (Create Entity Map): "If geometryProperty parameter is present and the
/// Accept Header is not set to "application/geo+json", then an error of type
/// `BadRequestData` shall be raised." The rule reads the named parameter and the
/// negotiated media type only, so an attribute the entity does not hold is
/// refused alike.
///
/// # Errors
///
/// [`ParameterError::GeometryPropertyWithoutGeoJson`] when `geometry_property`
/// is present and `media_type` is not `application/geo+json`.
pub const fn require_geojson_for_geometry_property(geometry_property: Option<&AttributeName>, media_type: ResponseMediaType) -> Result<(), ParameterError> {
    match (geometry_property, media_type) {
        (Some(_), ResponseMediaType::Json | ResponseMediaType::JsonLd) => Err(ParameterError::GeometryPropertyWithoutGeoJson(media_type)),
        (Some(_), ResponseMediaType::GeoJson) | (None, ResponseMediaType::Json | ResponseMediaType::JsonLd | ResponseMediaType::GeoJson) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::ParameterError, geometry_media_type::require_geojson_for_geometry_property};
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    fn location() -> AttributeName {
        "location".parse().expect("a valid attribute name")
    }

    #[test]
    fn a_geometry_property_without_a_geojson_accept_is_refused() {
        for media_type in [ResponseMediaType::Json, ResponseMediaType::JsonLd] {
            let refused = require_geojson_for_geometry_property(Some(&location()), media_type).expect_err("refused");
            assert!(matches!(refused, ParameterError::GeometryPropertyWithoutGeoJson(refused_for) if refused_for == media_type));
        }
    }

    #[test]
    fn a_geometry_property_with_a_geojson_accept_is_admitted() {
        assert!(require_geojson_for_geometry_property(Some(&location()), ResponseMediaType::GeoJson).is_ok());
    }

    #[test]
    fn no_geometry_property_is_admitted_under_every_media_type() {
        for media_type in [ResponseMediaType::Json, ResponseMediaType::JsonLd, ResponseMediaType::GeoJson] {
            assert!(require_geojson_for_geometry_property(None, media_type).is_ok());
        }
    }
}
