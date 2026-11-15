//! A temporal trailing path reads the bbox and the foreign members of a `GeoProperty`.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        EntityId,
        EntityTypeName,
        GeoProperty,
        NgsiLdMediaType,
        RawJson,
        evolution::TemporalEntity,
        geometry::{BoundingBox, Geometry, Position, Shape},
    },
    query::{q::Query, temporal_query},
};

/// The ids, sorted, of the Temporal Evolutions of `kiosk_type` the query `q` selects.
async fn selected(broker: &BrokerClient, kiosk_type: &EntityTypeName, q: &str) -> Result<Vec<String>> {
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(kiosk_type)
        .q(q.parse::<Query>()?)
        .send()
        .await?;
    let mut ids: Vec<String> = page.items().iter().map(|evolution| evolution.id.to_string()).collect();
    ids.sort();
    Ok(ids)
}

/// `ids` as sorted text.
fn sorted(ids: &[&EntityId]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
    ids.sort();
    ids
}

/// Records the Temporal Evolution of the Kiosk `id` with one `location` instance holding `geometry`, observed in 2020.
async fn record(broker: &BrokerClient, id: &EntityId, kiosk_type: &EntityTypeName, geometry: Geometry) -> Result<()> {
    let evolution = TemporalEntity::builder()
        .id(id.clone())
        .entity_type(kiosk_type.clone())
        .geo_property("location", GeoProperty::builder(geometry).observed_at("2020-01-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    Ok(())
}

/// Pins: for a Kiosk whose `location` instance is the Point 13.4, 52.5 written with the `bbox` 13.3, 52.4, 13.5, 52.6,
/// the foreign member `title` "Gate" and the foreign member `meta` holding `source` "survey", and a Kiosk whose
/// `location` instance is the same Point with no other member, each observed in 2020, Query Temporal Evolution of
/// Entities with q location[bbox], location[bbox]==13.3, location[title]=="Gate", location[meta.source]=="survey" and
/// location[meta]!="x" selects the first, with location[bbox]>0, location[title]=="Door" and location[missing] none,
/// and with location[type]=="Point" both; a temporal query reads a trailing path into a recorded geometry as Query
/// Entities reads it into the current one
/// Clause: 4.9, 4.7.1, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9387_a_temporal_trailing_path_reads_the_bbox_and_the_foreign_members_of_a_geoproperty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let kiosk_type = namespace.entity_type("Kiosk")?;
    let boxed_id = namespace.urn("Kiosk", "boxed")?;
    let bare_id = namespace.urn("Kiosk", "bare")?;
    let point = Shape::Point(Position::try_new(13.4, 52.5)?);
    let boxed = Geometry::from(point.clone())
        .with_bbox(BoundingBox::try_new(Position::try_new(13.3, 52.4)?, Position::try_new(13.5, 52.6)?)?)?
        .with_foreign_member("title".parse()?, r#""Gate""#.parse::<RawJson>()?)
        .with_foreign_member("meta".parse()?, r#"{"source":"survey"}"#.parse::<RawJson>()?);
    record(&broker, &boxed_id, &kiosk_type, boxed).await?;
    record(&broker, &bare_id, &kiosk_type, Geometry::from(point)).await?;
    let cases: [(&str, &[&EntityId]); 9] = [
        ("location[bbox]", &[&boxed_id]),
        ("location[bbox]==13.3", &[&boxed_id]),
        ("location[bbox]>0", &[]),
        ("location[title]==\"Gate\"", &[&boxed_id]),
        ("location[meta.source]==\"survey\"", &[&boxed_id]),
        ("location[title]==\"Door\"", &[]),
        ("location[meta]!=\"x\"", &[&boxed_id]),
        ("location[missing]", &[]),
        ("location[type]==\"Point\"", &[&boxed_id, &bare_id]),
    ];
    for (q, selects) in cases {
        expect_that!(selected(&broker, &kiosk_type, q).await?, eq(&sorted(selects)), "{q}");
    }
    Ok(())
}
