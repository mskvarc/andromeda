//! A scope set and removed inside the window matches it.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with `scopeQ` /Madrid/Centro between 12:00 and 13:00 selects a
/// Vehicle whose scope was set to it at 12:10 and removed at 12:40
/// Clause: 4.18, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9051_a_scope_set_and_removed_inside_the_window_matches_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_inside_window_id = namespace.urn("Vehicle", "inside-window")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_inside_window_evolution = TemporalEntity::builder()
        .id(vehicle_inside_window_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2018-08-01T12:20:00Z".parse()?))
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at scope), so `scope` is written as text.
    let scope: RawJson = r#"[{"type": "Property", "value": "/Madrid/Centro", "observedAt": "2018-08-01T12:10:00Z"}, {"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2018-08-01T12:40:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_inside_window_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_inside_window_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .scope_q("/Madrid/Centro".parse::<ScopeQuery>()?)
        .ids(vec![vehicle_inside_window_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_inside_window_id)));
    Ok(())
}
