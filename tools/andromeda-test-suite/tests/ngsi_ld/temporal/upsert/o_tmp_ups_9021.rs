//! A reified scope dated before a backfill holds over it.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Create Temporal Evolution of a Vehicle whose reified scope /<ns>/A is observed at 2024-01-01T00:00:00Z
/// beside speed observed at 00:10, 00:20 and 00:30 answers 201, and `scopeQ` /<ns>/A between 00:00 and 01:00
/// selects all three speed instances
/// Clause: 4.18, 4.5.6, 4.5.7, 5.6.11.4, 5.7.4.4, C.5.16
/// Recorded: clauses 5.6.11.4 and 5.6.12.4 do not say what a scope member of a Temporal Evolution Fragment records;
/// Andromeda records a reified scope instance as given, dated by its own `observedAt`
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9021_a_reified_scope_dated_before_a_backfill_holds_over_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_dated_backfill_id = namespace.urn("Vehicle", "dated-backfill")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_dated_backfill_evolution = TemporalEntity::builder()
        .id(vehicle_dated_backfill_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:10:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:20:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-01T00:30:00Z".parse()?))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.scope (written {"type":"Property","value":"/zqns/A","observedAt":"2024-01-01T00:00:00Z"})]: {"id":"urn:ngsi-ld:Vehicle:zqns-dated-backfill","type":"Vehicle_zqns","scope":[{"type":"Property","value":"/zqns/A","observedAt":"2024-01-01T00:00:00Z"}],"speed":[{"type":"Property","value":10,"observedAt":"2024-01-01T00:10:00Z"},{"type":"Property","value":20,"observedAt":"2024-01-01T00:20:00Z"},{"type":"Property","value":30,"observedAt":"2024-01-01T00:30:00Z"}]}), so `scope` is written as text.
    let scope: RawJson = format!(r#"{{"type": "Property", "value": "/{namespace}/A", "observedAt": "2024-01-01T00:00:00Z"}}"#).parse()?;
    let document = UncheckedDocument::try_from(vehicle_dated_backfill_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_dated_backfill_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-01T01:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .scope_q(format!("/{namespace}/A").as_str().parse::<ScopeQuery>()?)
        .ids(vec![vehicle_dated_backfill_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_evolution("speed", len(eq(3)))));
    Ok(())
}
