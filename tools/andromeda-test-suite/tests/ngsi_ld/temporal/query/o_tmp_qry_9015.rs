//! A rooted pattern selects a temporal scope without the slash.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with `scopeQ=/T<ns>` answers the Entity whose scope instance is T<ns>
/// Clause: 4.18, 4.19, 5.7.4
/// Recorded: 4.18 makes the leading slash of a Scope optional ("Madrid" is its example 2) and 4.19 writes every
/// pattern with one; Andromeda reads a Scope written without the slash as the same levels, so a rooted pattern
/// selects it
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9015_a_rooted_pattern_selects_a_temporal_scope_without_the_slash() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_bare_id = namespace.urn("Vehicle", "bare")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_bare_evolution = TemporalEntity::builder()
        .id(vehicle_bare_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2018-08-01T11:05:00Z".parse()?))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.scope (written {"type":"Property","value":"Tzqns","observedAt":"2018-08-01T11:00:00Z"})]: {"id":"urn:ngsi-ld:Vehicle:zqns-bare","type":"Vehicle_zqns","scope":[{"type":"Property","value":"Tzqns","observedAt":"2018-08-01T11:00:00Z"}],"speed":[{"type":"Property","value":10,"observedAt":"2018-08-01T11:05:00Z"}]}), so `scope` is written as text.
    let scope: RawJson = format!(r#"{{"type": "Property", "value": "T{namespace}", "observedAt": "2018-08-01T11:00:00Z"}}"#).parse()?;
    let document = UncheckedDocument::try_from(vehicle_bare_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_bare_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2018-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_bare_id.clone()])
        .scope_q(format!("/T{namespace}").as_str().parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_bare_id)));
    Ok(())
}
