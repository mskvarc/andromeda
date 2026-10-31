//! Several scopes are one instance holding the array.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Scope,
        evolution::{ScopeValue, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity whose one scope instance names /Madrid/Centro and
/// `/Company123/UnitA` answers one instance holding both
/// Clause: 4.18
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9033_several_scopes_are_one_instance_holding_the_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_several_id = namespace.urn("Vehicle", "several")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_several_evolution = TemporalEntity::builder()
        .id(vehicle_several_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(30).observed_at("2018-08-01T12:20:00Z".parse()?))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.scope (written {"type":"Property","value":["/Madrid/Centro","/Company123/UnitA"],"observedAt":"2018-08-01T12:10:00Z"})]: {"id":"urn:ngsi-ld:Vehicle:zqns-several","type":"Vehicle_zqns","scope":[{"type":"Property","value":["/Madrid/Centro","/Company123/UnitA"],"observedAt":"2018-08-01T12:10:00Z"}],"speed":[{"type":"Property","value":30,"observedAt":"2018-08-01T12:20:00Z"}]}), so `scope` is written as text.
    let scope: RawJson = r#"{"type": "Property", "value": ["/Madrid/Centro", "/Company123/UnitA"], "observedAt": "2018-08-01T12:10:00Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_several_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_several_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_several_id)
        .temporal_q(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.scope, len(eq(1)));
    expect_that!(
        evolution
            .scope
            .first()
            .and_then(|instance| if let ScopeValue::Scopes(scopes) = instance.value() {
                Some(scopes.as_slice())
            } else {
                None
            }),
        some(len(eq(2)))
    );
    expect_that!(
        evolution
            .scope
            .first()
            .and_then(|instance| if let ScopeValue::Scopes(scopes) = instance.value() {
                Some(scopes.as_slice())
            } else {
                None
            }),
        some(nth(0, eq(&"/Madrid/Centro".parse::<Scope>()?)))
    );
    expect_that!(
        evolution
            .scope
            .first()
            .and_then(|instance| if let ScopeValue::Scopes(scopes) = instance.value() {
                Some(scopes.as_slice())
            } else {
                None
            }),
        some(nth(1, eq(&"/Company123/UnitA".parse::<Scope>()?)))
    );
    Ok(())
}
