//! A trailing path reads the members of a geometry in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{
    Entity,
    geometry::{BoundingBox, Geometry, LineString, Position, Shape},
};

/// Pins: for a Kiosk whose `location` is the Point 13.4, 52.5 stored with a `bbox`, a Kiosk whose `location` is the
/// `LineString` from 13.4, 52.5 to 13.5, 52.6 and a Kiosk with no `location`, Query Entities and a Subscription with q
/// location[type]=="Point" select the first, location[type]=="Polygon" none, location[coordinates] the first two,
/// location[coordinates]==13.4 the first, location[coordinates]!=13.4 the second and location[bbox] none; a trailing
/// path is a member expression against the `GeoJSON` geometry object of the geometry, whose `type` member the core
/// context maps to a keyword, a stored `bbox` is no member of it, and a position is a single element of a `LineString`'s
/// coordinates that includes no number
/// Clause: 4.9, 4.7, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9388_a_trailing_path_reads_the_members_of_a_geometry_in_a_query_and_a_subscription_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let kiosk_type = namespace.entity_type("Kiosk")?;
    let placed_id = namespace.urn("Kiosk", "placed")?;
    let routed_id = namespace.urn("Kiosk", "routed")?;
    let unplaced_id = namespace.urn("Kiosk", "unplaced")?;
    let corner = || Position::try_new(13.4, 52.5);
    let boxed = Geometry::from(Shape::Point(corner()?)).with_bbox(BoundingBox::try_new(corner()?, corner()?)?)?;
    let route = Shape::LineString(LineString::try_new(vec![corner()?, Position::try_new(13.5, 52.6)?])?);
    let entities = [
        Entity::builder()
            .id(placed_id.clone())
            .entity_type(kiosk_type.clone())
            .geo_property("location", boxed)
            .build()?,
        Entity::builder()
            .id(routed_id.clone())
            .entity_type(kiosk_type.clone())
            .geo_property("location", route)
            .build()?,
        Entity::builder()
            .id(unplaced_id)
            .entity_type(kiosk_type.clone())
            .property("name", "kiosk")
            .build()?,
    ];
    let cases = [
        QCase {
            q: "location[type]==\"Point\"",
            selects: &[&placed_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "location[type]==\"Polygon\"",
            selects: &[],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "location[coordinates]",
            selects: &[&placed_id, &routed_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "location[coordinates]==13.4",
            selects: &[&placed_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "location[coordinates]!=13.4",
            selects: &[&routed_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "location[bbox]",
            selects: &[],
            request: QRequest::PLAIN,
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &kiosk_type, &entities, &cases).await
}
