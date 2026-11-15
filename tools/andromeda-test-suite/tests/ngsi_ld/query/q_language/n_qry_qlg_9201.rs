//! A trailing path reads the bbox and the foreign members of a `GeoProperty`.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{
    Entity,
    EntityId,
    RawJson,
    geometry::{BoundingBox, Geometry, Position, Shape},
};

/// The term `q` selecting `selects`, asked with no other request member.
const fn case<'c>(q: &'c str, selects: &'c [&'c EntityId]) -> QCase<'c> {
    QCase {
        q,
        selects,
        request: QRequest::PLAIN,
    }
}

/// Pins: for a Kiosk whose `location` is the Point 13.4, 52.5 written with the `bbox` 13.3, 52.4, 13.5, 52.6, the
/// foreign member `title` "Gate" and the foreign member `meta` holding `source` "survey", a Kiosk whose `location` is
/// the same Point with no other member and a Kiosk with no `location`, Query Entities and a Subscription with q
/// location[bbox] and location[bbox]==13.3 select the first, location[bbox]>0 none, location[title]=="Gate" and
/// location[meta.source]=="survey" the first, location[title]=="Door" none, location[meta]!="x" the first,
/// location[missing] none, and location[type]=="Point" and location[coordinates]==13.4 the first two; a trailing path
/// is a member expression against the `GeoJSON` geometry object, whose `bbox` is an array the equality operators read
/// by membership and no ordering matches, and whose foreign members, a structured value among them, are defined
/// values, a structured value being unequal to a String
/// Clause: 4.9, 4.7.1, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9201_a_trailing_path_reads_the_bbox_and_the_foreign_members_of_a_geoproperty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let kiosk_type = namespace.entity_type("Kiosk")?;
    let boxed_id = namespace.urn("Kiosk", "boxed")?;
    let bare_id = namespace.urn("Kiosk", "bare")?;
    let unplaced_id = namespace.urn("Kiosk", "unplaced")?;
    let point = Shape::Point(Position::try_new(13.4, 52.5)?);
    let boxed = Geometry::from(point.clone())
        .with_bbox(BoundingBox::try_new(Position::try_new(13.3, 52.4)?, Position::try_new(13.5, 52.6)?)?)?
        .with_foreign_member("title".parse()?, r#""Gate""#.parse::<RawJson>()?)
        .with_foreign_member("meta".parse()?, r#"{"source":"survey"}"#.parse::<RawJson>()?);
    let entities = [
        Entity::builder()
            .id(boxed_id.clone())
            .entity_type(kiosk_type.clone())
            .geo_property("location", boxed)
            .build()?,
        Entity::builder()
            .id(bare_id.clone())
            .entity_type(kiosk_type.clone())
            .geo_property("location", point)
            .build()?,
        Entity::builder()
            .id(unplaced_id)
            .entity_type(kiosk_type.clone())
            .property("name", "kiosk")
            .build()?,
    ];
    let (boxed_only, both) = ([&boxed_id], [&boxed_id, &bare_id]);
    let cases = [
        case("location[bbox]", &boxed_only),
        case("location[bbox]==13.3", &boxed_only),
        case("location[bbox]>0", &[]),
        case("location[title]==\"Gate\"", &boxed_only),
        case("location[meta.source]==\"survey\"", &boxed_only),
        case("location[title]==\"Door\"", &[]),
        case("location[meta]!=\"x\"", &boxed_only),
        case("location[missing]", &[]),
        case("location[type]==\"Point\"", &both),
        case("location[coordinates]==13.4", &both),
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &kiosk_type, &entities, &cases).await
}
