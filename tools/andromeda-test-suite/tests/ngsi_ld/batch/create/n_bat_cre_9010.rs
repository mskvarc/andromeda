//! A batch creation refuses only the entity holding location as a property.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{
        Entity,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
};

/// Pins: Batch Entity Creation of an Entity holding `location` as a `GeoProperty` and one holding it as a Property
/// answers 207 with the first a success and the second an error of type `BadRequestData` naming `location`, and
/// only the first is stored
/// Clause: 5.2.4, 5.5.4, 5.6.7.4
#[gtest]
#[tokio::test]
async fn n_bat_cre_9010_a_batch_creation_refuses_only_the_entity_holding_location_as_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    let located_id = namespace.urn("GeoMemberProbe", "located")?;
    let misfiled_id = namespace.urn("GeoMemberProbe", "misfiled")?;
    let located = Entity::builder()
        .id(located_id.clone())
        .entity_type(probe_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    let misfiled = Entity::builder().id(misfiled_id.clone()).entity_type(probe_type).build()?;
    // The model admits `location` only as a GeoProperty, so it is written as text.
    let location: RawJson = r#"{"type": "Property", "value": "somewhere"}"#.parse()?;
    let documents = [
        UncheckedDocument::from_entity(&located, Lossless::Normalized),
        UncheckedDocument::from_entity(&misfiled, Lossless::Normalized).set_member("location", &location),
    ];
    let entities = [located, misfiled];
    let request = broker.unchecked(&BatchCreate::builder().entities(&entities).build())?.documents(&documents);
    let report: BatchOperationResult = reported(broker.send_unchecked(request).await)?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&located_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(
            0,
            batch_error()
                .entity(&misfiled_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'location' must be a GeoProperty")
        )
    );
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    json.entities().retrieve(&located_id).send().await?;
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&misfiled_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    Ok(())
}
