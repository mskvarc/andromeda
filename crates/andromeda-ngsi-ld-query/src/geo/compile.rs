//! Compilation of the geo query parameter group into a [`GeoQuery`].

use crate::{error::QueryError, geo::query::GeoQuery, term_expander::TermExpander};
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};

/// The GeoProperty a geo query addresses when the request names none.
const DEFAULT_GEOPROPERTY: &str = "location";

/// Compile geo query parameters into a [`GeoQuery`].
///
/// All three of `georel`, `geometry`, `coordinates` must be present together or
/// all absent (clause 4.10). `geoproperty` defaults to `location`.
///
/// `geometry` and `coordinates` are the two halves of one GeoJSON geometry
/// object (clause 4.10, IETF RFC 7946), so they are rejoined and parsed here.
pub async fn compile_geo_query(
    georel: Option<&str>,
    geometry: Option<&str>,
    coordinates: Option<&str>,
    geoproperty: Option<&str>,
    expander: &impl TermExpander,
) -> Result<Option<GeoQuery>, QueryError> {
    let (georel_str, geometry_str, coordinates_str) = match (georel, geometry, coordinates) {
        (Some(georel), Some(geometry), Some(coordinates)) => (georel, geometry, coordinates),
        (None, None, None) => return Ok(None),
        (None | Some(_), None | Some(_), None | Some(_)) => return Err(QueryError::GeoIncomplete),
    };

    let geoproperty_iri = expander.expand_term(geoproperty.unwrap_or(DEFAULT_GEOPROPERTY)).await?;
    let coordinates_value: serde_json::Value = serde_json::from_str(coordinates_str).map_err(QueryError::GeoCoordinatesInvalid)?;

    Ok(Some(GeoQuery {
        georel: georel_str.parse::<GeoRel>()?,
        geometry: reference_geometry(geometry_str, coordinates_value)?,
        geoproperty_iri,
    }))
}

/// Rejoin the `geometry` and `coordinates` parameters into one GeoJSON
/// geometry object and parse it.
fn reference_geometry(geometry_type: &str, coordinates: serde_json::Value) -> Result<Geometry, QueryError> {
    let geojson = serde_json::json!({
        "type": geometry_type,
        "coordinates": coordinates
    });
    Ok(Geometry::from_geojson_value(&geojson)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iri_rs::IriBuf;

    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        async fn expand_term(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI"))
        }

        async fn expand_type(&self, term: &str) -> Result<IriBuf, QueryError> {
            Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI"))
        }
    }

    #[tokio::test]
    async fn all_three_components_absent_is_no_geo_query() {
        assert!(
            compile_geo_query(None, None, None, None, &PrefixExpander)
                .await
                .expect("no geo query")
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_partial_component_set_is_rejected() {
        let error = compile_geo_query(Some("within"), None, None, None, &PrefixExpander)
            .await
            .expect_err("must reject");
        assert!(matches!(error, QueryError::GeoIncomplete), "got {error:?}");
    }

    #[tokio::test]
    async fn the_geoproperty_defaults_to_location() {
        let compiled = compile_geo_query(Some("within"), Some("Point"), Some("[1.0, 2.0]"), None, &PrefixExpander)
            .await
            .expect("compiles")
            .expect("present");
        assert_eq!(compiled.geoproperty_iri.as_str(), "https://example.com/location");
    }

    #[tokio::test]
    async fn unparseable_coordinates_are_rejected() {
        let error = compile_geo_query(Some("within"), Some("Point"), Some("not json"), None, &PrefixExpander)
            .await
            .expect_err("must reject");
        assert!(matches!(error, QueryError::GeoCoordinatesInvalid(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn an_unknown_georel_is_rejected() {
        let error = compile_geo_query(Some("beside"), Some("Point"), Some("[1.0, 2.0]"), None, &PrefixExpander)
            .await
            .expect_err("must reject");
        assert!(matches!(error, QueryError::GeoRel(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn a_geo_query_with_an_unparseable_geometry_fails_at_compile_time() {
        // Clause 4.10: `coordinates` must be coherent with `geometry` per IETF
        // RFC 7946. A single-element position is not a Point, and the failure
        // belongs to the request rather than to SQL generation, so it is raised
        // here and no query is ever built from it.
        let error = compile_geo_query(Some("within"), Some("Point"), Some("[1]"), None, &PrefixExpander)
            .await
            .expect_err("must reject");
        assert!(matches!(error, QueryError::ReferenceGeometry(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn an_unknown_geometry_type_fails_at_compile_time() {
        let error = compile_geo_query(Some("within"), Some("Trapezoid"), Some("[1.0, 2.0]"), None, &PrefixExpander)
            .await
            .expect_err("must reject");
        assert!(matches!(error, QueryError::ReferenceGeometry(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn a_polygon_ring_must_arrive_closed_and_wrapped() {
        // RFC 7946 section 3.1.6: Polygon coordinates are an array of linear
        // rings, and a linear ring's first and last positions are identical.
        // A bare, unclosed ring is not a Polygon and is rejected rather than
        // repaired.
        let bare_ring = "[[0.0,0.0],[1.0,0.0],[1.0,1.0]]";
        let error = compile_geo_query(Some("within"), Some("Polygon"), Some(bare_ring), None, &PrefixExpander)
            .await
            .expect_err("must reject");
        assert!(matches!(error, QueryError::ReferenceGeometry(_)), "got {error:?}");

        let closed_ring = "[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,0.0]]]";
        assert!(
            compile_geo_query(Some("within"), Some("Polygon"), Some(closed_ring), None, &PrefixExpander)
                .await
                .expect("compiles")
                .is_some()
        );
    }
}
