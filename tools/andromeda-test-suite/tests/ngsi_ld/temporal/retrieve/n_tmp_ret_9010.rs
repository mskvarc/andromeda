//! Each scope of a temporal evolution reads back as written.

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
};

/// Pins: Retrieve Temporal Evolution of an Entity whose scope instance holds a Scope with a leading slash and one
/// without answers that instance with both as written
/// Clause: 4.6.4, 4.18, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9010_each_scope_of_a_temporal_evolution_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_spelled_id = namespace.urn("Vehicle", "spelled")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_spelled_evolution = TemporalEntity::builder()
        .id(vehicle_spelled_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2018-08-01T11:05:00Z".parse()?))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.scope (written {"type":"Property","value":["/Tzqns/Gardens","Tzqns"],"observedAt":"2018-08-01T11:00:00Z"})]: {"id":"urn:ngsi-ld:Vehicle:zqns-spelled","type":"Vehicle_zqns","scope":[{"type":"Property","value":["/Tzqns/Gardens","Tzqns"],"observedAt":"2018-08-01T11:00:00Z"}],"speed":[{"type":"Property","value":10,"observedAt":"2018-08-01T11:05:00Z"}]}), so `scope` is written as text.
    let scope: RawJson =
        format!(r#"{{"type": "Property", "value": ["/T{namespace}/Gardens", "T{namespace}"], "observedAt": "2018-08-01T11:00:00Z"}}"#).parse()?;
    let document = UncheckedDocument::try_from(vehicle_spelled_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_spelled_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_spelled_id)
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
        some(nth(0, eq(&format!("/T{namespace}/Gardens").parse::<Scope>()?)))
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
        some(nth(1, eq(&format!("T{namespace}").parse::<Scope>()?)))
    );
    Ok(())
}
