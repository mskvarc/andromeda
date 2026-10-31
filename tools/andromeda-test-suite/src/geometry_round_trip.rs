//! The round trip of a `GeoProperty` through every representation and geo-query (clauses 4.5.4, 4.5.16, 4.7.1, 4.10):
//! a geometry written by Create Entity is read back, as written, normalized, concise, simplified and as a `GeoJSON`
//! Feature, is held by the area that contains it and not by the others; after Update Attributes replaces it, the
//! same reads give the new geometry and the areas swap.

use crate::{
    attribute_matcher::geo_property,
    broker::BrokerClient,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        EntityFragment,
        EntityId,
        EntityTypeName,
        Lossless,
        NgsiLdMediaType,
        SimplifiedValue,
        geometry::{Geometry, GeometryError, LineString, LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};
use std::result::Result as StdResult;

/// The geometries and areas of one round trip.
#[derive(Debug, Clone)]
pub struct GeometryTrip {
    /// The geometry the entity is created with.
    pub first: Shape,
    /// The geometry that replaces it.
    pub second: Shape,
    /// A polygon that contains `first` and not `second`.
    pub first_area: Shape,
    /// A polygon that contains `second` and not `first`.
    pub second_area: Shape,
    /// A polygon that contains neither.
    pub far_area: Shape,
}

/// The positions of `points`, each a longitude and a latitude.
///
/// # Errors
///
/// Returns [`GeometryError`] when a point is not a position.
pub fn positions(points: &[(f64, f64)]) -> StdResult<Vec<Position>, GeometryError> {
    points.iter().map(|(longitude, latitude)| Position::try_new(*longitude, *latitude)).collect()
}

/// The line string through `points`.
///
/// # Errors
///
/// Returns [`GeometryError`] when the points are not a line string.
pub fn line(points: &[(f64, f64)]) -> StdResult<LineString, GeometryError> {
    LineString::try_new(positions(points)?)
}

/// The polygon whose rings are `rings`, the first the exterior and the others holes.
///
/// # Errors
///
/// Returns [`GeometryError`] when a ring is not closed or the polygon is not valid.
pub fn polygon(rings: &[&[(f64, f64)]]) -> StdResult<Polygon, GeometryError> {
    Polygon::try_new(
        rings
            .iter()
            .map(|ring| LinearRing::try_new(positions(ring)?))
            .collect::<StdResult<Vec<_>, GeometryError>>()?,
    )
}

/// The rectangle between two corners, as a polygon.
///
/// # Errors
///
/// Returns [`GeometryError`] when the corners do not make a polygon.
pub fn rectangle(west: f64, south: f64, east: f64, north: f64) -> StdResult<Shape, GeometryError> {
    Ok(Shape::Polygon(polygon(&[&[
        (west, south),
        (east, south),
        (east, north),
        (west, north),
        (west, south),
    ]])?))
}

/// Runs `trip` on `client`'s tenant with an entity of a type of the test's namespace.
///
/// # Errors
///
/// Returns the failure of the first request the broker refuses or answers unreadably; a read or a query that gives
/// another answer than the trip states is an assertion failure.
pub async fn run(client: &BrokerClient, namespace: &Namespace, trip: &GeometryTrip) -> Result<()> {
    let spot_type = namespace.entity_type("Spot")?;
    let spot_id = namespace.urn("Spot", 1)?;
    let location = "location".parse::<AttributeName>()?;
    let spot = Entity::builder()
        .id(spot_id.clone())
        .entity_type(spot_type.clone())
        .geo_property("location", trip.first.clone())
        .build()?;
    client.entities().create(&spot).send().await?.done()?;
    reads(client, &spot_id, &location, &trip.first).await?;
    held(client, &spot_type, &trip.first_area, Some(&spot_id)).await?;
    held(client, &spot_type, &trip.second_area, None).await?;
    held(client, &spot_type, &trip.far_area, None).await?;
    let fragment = EntityFragment::builder().geo_property("location", trip.second.clone()).build()?;
    client.entities().update_attributes(&spot_id, &fragment).send().await?.done()?;
    reads(client, &spot_id, &location, &trip.second).await?;
    held(client, &spot_type, &trip.first_area, None).await?;
    held(client, &spot_type, &trip.second_area, Some(&spot_id)).await?;
    Ok(())
}

/// Reads the entity in the four representations and expects `shape` as the location of each.
async fn reads(client: &BrokerClient, id: &EntityId, location: &AttributeName, shape: &Shape) -> Result<()> {
    let json = client.with_accept(Accept::new(NgsiLdMediaType::Json));
    let normalized = json.entities().retrieve(id).send().await?;
    expect_that!(normalized.body(), has_attribute("location", geo_property().geometry(shape.clone())));
    let concise = json.entities().retrieve(id).form(Lossless::Concise).send().await?;
    expect_that!(concise.body(), has_attribute("location", geo_property().geometry(shape.clone())));
    let simplified = json.entities().retrieve_simplified(id).send().await?;
    expect_that!(
        simplified.body().attributes.get(location),
        some(eq(&SimplifiedValue::Geometry(Geometry::from(shape.clone()))))
    );
    let feature = client
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(id)
        .geometry_property(location)
        .send()
        .await?;
    expect_that!(feature.body().entity.id, eq(id));
    expect_that!(feature.body().geometry.as_ref(), some(eq(&Geometry::from(shape.clone()))));
    Ok(())
}

/// Queries the entities of `entity_type` within `area` and expects the entity `held` and no other, or none.
async fn held(client: &BrokerClient, entity_type: &EntityTypeName, area: &Shape, held: Option<&EntityId>) -> Result<()> {
    let page = client
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(entity_type)
        .geo_q(geoquery::within(area.clone()))
        .send()
        .await?;
    match held {
        Some(id) => {
            expect_that!(page.items(), len(eq(1)));
            expect_that!(page.items(), contains(has_id(id)));
        }
        None => expect_that!(page.items(), is_empty()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::geometry_round_trip::{line, polygon, rectangle};
    use googletest::prelude::*;
    use ngsi_ld::model::geometry::Shape;

    #[gtest]
    fn a_rectangle_is_a_polygon_of_one_closed_ring() -> Result<()> {
        let Shape::Polygon(_) = rectangle(0.0, 0.0, 2.0, 1.0)? else {
            return fail!("the rectangle is not a polygon");
        };
        Ok(())
    }

    #[gtest]
    fn a_polygon_with_a_hole_keeps_both_rings() -> Result<()> {
        let outer: &[(f64, f64)] = &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        let hole: &[(f64, f64)] = &[(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0), (2.0, 2.0)];
        verify_that!(polygon(&[outer, hole]).is_ok(), eq(true))
    }

    #[gtest]
    fn a_line_of_one_point_is_refused() -> Result<()> {
        verify_that!(line(&[(1.0, 1.0)]).is_err(), eq(true))
    }
}
