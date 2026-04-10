use crate::{
    error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine},
    query::compiled::{GeoQuery, GeoRel},
};

/// Compile geo query parameters into a [`GeoQuery`].
///
/// All three of `georel`, `geometry`, `coordinates` must be present together or
/// all absent.  `geoproperty` defaults to `"location"` when not supplied.
pub async fn compile_geo_query(
    georel: Option<&str>,
    geometry: Option<&str>,
    coordinates: Option<&str>,
    geoproperty: Option<&str>,
    engine: &JsonLdEngine,
    context: &ContextSpec,
) -> Result<Option<GeoQuery>, AndromedaError> {
    let (georel_str, geometry_str, coordinates_str) = match (georel, geometry, coordinates) {
        (Some(gr), Some(geom), Some(coords)) => (gr, geom, coords),
        (None, None, None) => return Ok(None),
        _ => {
            return Err(AndromedaError::bad_request_data(
                "georel, geometry, and coordinates must all be provided together",
            ));
        }
    };

    let geoproperty = geoproperty.unwrap_or("location");
    let geoproperty_iri = engine.expand_term(geoproperty, context).await?;

    let coordinates_value: serde_json::Value =
        serde_json::from_str(coordinates_str).map_err(|e| AndromedaError::bad_request_data(format!("Invalid coordinates JSON: {}", e)))?;

    Ok(Some(GeoQuery {
        georel: GeoRel::parse(georel_str)?,
        geometry: geometry_str.to_string(),
        coordinates: coordinates_value,
        geoproperty_iri,
    }))
}
