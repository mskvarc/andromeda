//! A scope query narrows the candidates to the entities in the scope.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
    query::{scope_query::ScopeQuery, temporal_query},
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with `scopeQ` naming /Madrid/Center maps the
/// Entity whose scope is /Madrid/Center and leaves out the one in /Barcelona, and without `scopeQ` maps both
/// Clause: 5.14.5.4, 4.19
#[gtest]
#[tokio::test]
async fn n_map_qte_7009_a_scope_query_narrows_the_candidates_to_the_entities_in_the_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_inside_id = namespace.urn("Vehicle", "inside")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_outside_id = namespace.urn("Vehicle", "outside")?;
    let vehicle_inside_evolution = TemporalEntity::builder()
        .id(vehicle_inside_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    // The model refuses it (a temporal scope), so `scope` is written as text.
    let scope: RawJson = r#"[{"type": "Property", "value": "/Madrid/Center", "observedAt": "2021-01-01T00:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_inside_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_inside_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let vehicle_outside_evolution = TemporalEntity::builder()
        .id(vehicle_outside_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    // The model refuses it (a temporal scope), so `scope` is written as text.
    let scope_2: RawJson = r#"[{"type": "Property", "value": "/Barcelona", "observedAt": "2021-01-01T00:00:00Z"}]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(vehicle_outside_evolution.render(Lossless::Normalized))?.set_member("scope", &scope_2);
    let request_2 = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_outside_evolution).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .scope_q("/Madrid/Center".parse::<ScopeQuery>()?)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(1)));
    expect_that!(map.entity_map.get(&vehicle_inside_id), some(contains(eq(&EntitySource::Local))));
    let created_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_2 = created_2.body();
    expect_that!(&map_2.entity_map, len(eq(2)));
    Ok(())
}
