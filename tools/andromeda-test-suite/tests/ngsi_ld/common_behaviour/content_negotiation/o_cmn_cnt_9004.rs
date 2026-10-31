//! An entity a query answer and a temporal evolution open with the context.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity, Query Entities and Retrieve Temporal Evolution of an Entity answered as
/// application/ld+json open with @context, the query on its item
/// Clause: 6.3.6, 5.7.1, 5.7.2, 5.7.3
/// Recorded: 6.3.6 requires the @context in an application/ld+json body and fixes no member order (RFC 8259 objects
/// are unordered); Andromeda writes it as the first member, of every item of a collection too
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_9004_an_entity_a_query_answer_and_a_temporal_evolution_open_with_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(7).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let json_ld = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let retrieved = json_ld.entities().retrieve(&vehicle_id).send().await?;
    expect_that!(String::from_utf8_lossy(&retrieved.body).as_ref(), matches_regex(r#"(?s)^\s*\{\s*"@context".*"#));
    let queried = json_ld.entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(queried.outcome.items(), len(eq(1)));
    expect_that!(
        String::from_utf8_lossy(&queried.body).as_ref(),
        matches_regex(r#"(?s)^\s*\[\s*\{\s*"@context".*"#)
    );
    let evolution = json_ld.temporal().retrieve(&vehicle_id).send().await?;
    expect_that!(String::from_utf8_lossy(&evolution.body).as_ref(), matches_regex(r#"(?s)^\s*\{\s*"@context".*"#));
    Ok(())
}
