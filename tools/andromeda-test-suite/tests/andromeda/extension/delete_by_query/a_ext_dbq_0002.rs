//! A dry run reports the count and removes nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::answered,
    temporal_delete_by_query::{delete_request, dry_run_count},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: a temporal delete-by-query with `dryRun=true` answers 200 with the number of instances the window matches
/// in deleted, and removes nothing
/// Clause: 4.11, 6.18.3
/// Recorded: Andromeda extension, not v1.9.1: clause 6.18.3 binds no DELETE to the temporal entities collection;
/// the `temporal_delete_by_query` extension adds it, here with at most 2 matched instances
#[gtest]
#[tokio::test]
async fn a_ext_dbq_0002_a_dry_run_reports_the_count_and_removes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-03-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // No operation of the specification deletes temporal evolutions by a query, so the request is sent as written.
    let request = delete_request(
        &broker,
        &format!(
            "id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&drop=speed&timerel=between&timeAt=2024-01-01T00%3A00%3A00Z&endTimeAt=2024-06-01T00%3A00%3A00Z&dryRun=true"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    expect_that!(dry_run_count(answer.as_ref().or_fail()?)?, eq(2));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                nth(0, any_attribute().observed_at("2024-01-01T00:00:00Z".parse()?)),
                nth(1, any_attribute().observed_at("2024-03-01T00:00:00Z".parse()?)),
                nth(2, any_attribute().observed_at("2024-06-01T00:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
