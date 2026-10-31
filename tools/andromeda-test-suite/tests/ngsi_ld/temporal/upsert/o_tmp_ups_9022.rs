//! An undated scope restated by a backfill is one scope instance at the first write.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{
        Lossless,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        RawJson,
        Scope,
        evolution::{ScopeInstance, ScopeValue, TemporalEntity},
    },
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Two Upsert Temporal Evolution requests for one Vehicle, each carrying the non-reified scope /<ns>/A beside
/// speed observed on 2024-01-01, leave one scope instance in its Temporal Evolution, and `scopeQ` /<ns>/A between
/// 2024-01-01T00:00:00Z and 01:00 selects no Entity
/// Clause: 4.18, 4.5.6, 5.6.11.4, 5.7.4.4, C.5.16
/// Recorded: clauses 5.6.11.4 and 5.6.12.4 do not say what a scope member of a Temporal Evolution Fragment records;
/// Andromeda reads a non-reified scope as stating at the write that the Entity is in that Scope, which records
/// nothing when the Scope is already in force, so the Scope holds from the first write and over none of the
/// instances it was sent with
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9022_an_undated_scope_restated_by_a_backfill_is_one_scope_instance_at_the_first_write() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_undated_backfill_id = namespace.urn("Vehicle", "undated-backfill")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_undated_backfill_evolution = TemporalEntity::builder()
        .id(vehicle_undated_backfill_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:10:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:20:00Z".parse()?))
        .build()?;
    // The model refuses it (expected a JSON object at scope), so `scope` is written as text.
    let scope: RawJson = format!(r#""/{namespace}/A""#).parse()?;
    let document = UncheckedDocument::try_from(vehicle_undated_backfill_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_undated_backfill_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let vehicle_undated_backfill_evolution_2 = TemporalEntity::builder()
        .id(vehicle_undated_backfill_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2024-01-01T00:30:00Z".parse()?))
        .build()?;
    // The model refuses it (expected a JSON object at scope), so `scope` is written as text.
    let scope_2: RawJson = format!(r#""/{namespace}/A""#).parse()?;
    let document_2 = UncheckedDocument::try_from(vehicle_undated_backfill_evolution_2.render(Lossless::Normalized))?.set_member("scope", &scope_2);
    let request_2 = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_undated_backfill_evolution_2).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_undated_backfill_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.scope, len(eq(1)));
    expect_that!(
        evolution.scope.first().map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one(format!("/{namespace}/A").parse::<Scope>()?))))
    );
    expect_that!(evolution, has_evolution("speed", len(eq(3))));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-01T01:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .scope_q(format!("/{namespace}/A").as_str().parse::<ScopeQuery>()?)
        .ids(vec![vehicle_undated_backfill_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
