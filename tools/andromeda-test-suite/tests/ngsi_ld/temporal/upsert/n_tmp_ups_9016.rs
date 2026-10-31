//! A reified scope within the grammar is recorded.

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

/// Pins: Create Temporal Evolution of an Entity whose reified scope holds /Madrid/Gardens and `/CompanyA` answers
/// 201, and the Temporal Evolution holds one scope instance with both
/// Clause: 4.18, 4.5.6, 5.6.11.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9016_a_reified_scope_within_the_grammar_is_recorded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_reified_valid_id = namespace.urn("Vehicle", "reified-valid")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_reified_valid_evolution = TemporalEntity::builder()
        .id(vehicle_reified_valid_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(80).observed_at("2020-06-01T00:00:00Z".parse()?))
        .build()?;
    // The model refuses it (a temporal scope), so `scope` is written as text.
    let scope: RawJson = r#"[{"type": "Property", "value": ["/Madrid/Gardens", "/CompanyA"], "observedAt": "2020-06-01T00:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_reified_valid_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_reified_valid_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_reified_valid_id)
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
        some(nth(0, eq(&"/Madrid/Gardens".parse::<Scope>()?)))
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
        some(nth(1, eq(&"/CompanyA".parse::<Scope>()?)))
    );
    Ok(())
}
