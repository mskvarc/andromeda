//! Any instance of the target geo property is enough to match.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{
        Entity,
        GeoProperty,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};

/// Pins: A geo-query matches an entity when any instance of the target `GeoProperty` meets it, a dataset instance
/// as much as the default one, and an entity none of whose instances meets it does not match
/// Clause: 4.10, 4.5.5, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_0015_any_instance_of_the_target_geo_property_is_enough_to_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_only_tagged_id = namespace.urn("Building", "only-tagged")?;
    let building_type = namespace.entity_type("Building")?;
    let building_default_out_tagged_in_id = namespace.urn("Building", "default-out-tagged-in")?;
    let building_both_out_id = namespace.urn("Building", "both-out")?;
    let building_only_tagged = Entity::builder()
        .id(building_only_tagged_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.location (written [{"type":"GeoProperty","value":{"type":"Point","coordinates":[13.4,52.52]},"datasetId":"urn:ngsi-ld:Dataset:zqns-gps"}])]: {"id":"urn:ngsi-ld:Building:zqns-only-tagged","type":"Building_zqns","location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[13.4,52.52]},"datasetId":"urn:ngsi-ld:Dataset:zqns-gps"}}), so `location` is written as text.
    let location: RawJson = format!(
        r#"[{{"type": "GeoProperty", "value": {{"type": "Point", "coordinates": [13.4, 52.52]}}, "datasetId": "urn:ngsi-ld:Dataset:{namespace}-gps"}}]"#
    )
    .parse()?;
    let document = UncheckedDocument::from_entity(&building_only_tagged, Lossless::Normalized).set_member("location", &location);
    let building_default_out_tagged_in = Entity::builder()
        .id(building_default_out_tagged_in_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(20.0, 40.0)?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.52)?)).dataset_id(namespace.dataset_id("Dataset", "gps")?),
        )
        .build()?;
    let document_2 = UncheckedDocument::from_entity(&building_default_out_tagged_in, Lossless::Normalized);
    let building_both_out = Entity::builder()
        .id(building_both_out_id)
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(20.0, 40.0)?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(21.0, 41.0)?)).dataset_id(namespace.dataset_id("Dataset", "gps")?),
        )
        .build()?;
    let document_3 = UncheckedDocument::from_entity(&building_both_out, Lossless::Normalized);
    let entities = [building_only_tagged, building_default_out_tagged_in, building_both_out];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2, document_3]);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // A dataset instance inside is enough; both-out has no instance inside.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.0, 52.0)?,
            Position::try_new(14.0, 52.0)?,
            Position::try_new(14.0, 53.0)?,
            Position::try_new(13.0, 53.0)?,
            Position::try_new(13.0, 52.0)?,
        ])?])?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_only_tagged_id)));
    expect_that!(page.items(), contains(has_id(&building_default_out_tagged_in_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(13.4, 52.52)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_only_tagged_id)));
    expect_that!(page_2.items(), contains(has_id(&building_default_out_tagged_in_id)));
    Ok(())
}
