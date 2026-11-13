//! A batch merge refuses location as a relationship and keeps the stored one.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchMerge},
    model::{
        Entity,
        EntityFragment,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Batch Entity Merge writing `location` as a Relationship onto an Entity holding it as a `GeoProperty` answers
/// 207 with an error of type `BadRequestData` naming `location` for that Entity, and `location` keeps the position
/// it held
/// Clause: 5.2.4, 5.5.4, 5.6.20.4
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9011_a_batch_merge_refuses_location_as_a_relationship_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    let probe_id = namespace.urn("GeoMemberProbe", "merged")?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(probe_id.clone(), probe_type).build()?];
    // The model admits `location` only as a GeoProperty, so it is written as text.
    let location: RawJson = r#"{"type": "Relationship", "object": "urn:ngsi-ld:Place:somewhere"}"#.parse()?;
    let document = UncheckedDocument::try_from(fragments[0].render(Lossless::Normalized))?.set_member("location", &location);
    let request = broker.unchecked(&BatchMerge::builder().fragments(&fragments).build())?.documents(&[document]);
    let report: BatchOperationResult = reported(broker.send_unchecked(request).await)?;
    expect_that!(&report.success, len(eq(0)));
    expect_that!(
        &report.errors,
        nth(
            0,
            batch_error()
                .entity(&probe_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'location' must be a GeoProperty")
        )
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&probe_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute("location", any_attribute().value_member(["coordinates", "0"], Value::Number("14.5".parse()?)))
    );
    Ok(())
}
