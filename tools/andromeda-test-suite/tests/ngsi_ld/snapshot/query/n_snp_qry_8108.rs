//! A retrieval under a snapshot renders every representation.

use andromeda_test_suite::{
    attribute_matcher::{geo_property, property},
    broker,
    entity_matcher::has_attribute,
    eventually::retried,
    geometry_matcher::geometry,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, has_value_at},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Retrieve Entity with NGSILD-Snapshot renders the normalized representation by default and with options
/// normalized, the concise and `keyValues` representations on request, and Query Entities with Accept
/// application/geo+json answers a `FeatureCollection` of the Entity
/// Clause: 5.5.15, 4.5.3, 4.5.4, 5.7.1.4, 6.3.15
#[gtest]
#[tokio::test]
async fn n_snp_qry_8108_a_retrieval_under_a_snapshot_renders_every_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let captured = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(captured.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let location = Shape::Point(Position::try_new(13.0, 46.0)?);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 8)
        .geo_property("location", location.clone())
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let by_default = json.on_snapshot(&snapshot_id).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(by_default.body(), has_attribute("speed", property().value(8)));
    let normalized = json
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .spelling(RepresentationSpelling::default().in_options().explicit_normalized())
        .send()
        .await?;
    expect_that!(normalized.body(), has_attribute("speed", property().value(8)));
    let concise = json
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve(&vehicle_id)
        .form(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(concise.body(), has_attribute("speed", property().value(8)));
    expect_that!(concise.body(), has_attribute("location", geo_property().geometry(location)));
    let simplified = json
        .on_snapshot(&snapshot_id)
        .entities()
        .retrieve_simplified(&vehicle_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(simplified.body(), has_value("speed", 8));
    expect_that!(simplified.body(), has_value_at("location", ["type"], "Point"));
    let features = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .on_snapshot(&snapshot_id)
        .entities()
        .query_geo_json_simplified()
        .entity_type(&vehicle_type)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(features.items(), len(eq(1)));
    let feature = features.items().first().or_fail()?;
    expect_that!(feature.entity.id, eq(&vehicle_id));
    expect_that!(&feature.entity, has_value("speed", 8));
    expect_that!(feature.geometry.as_ref(), some(geometry().of_type("Point")));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
