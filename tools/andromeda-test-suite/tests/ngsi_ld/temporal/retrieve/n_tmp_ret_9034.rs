//! A later scope supersedes the one before it.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
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

/// Pins: Retrieve Temporal Evolution of an Entity whose scope was /Madrid/Centro from 09:00 and `/Company123/UnitA`
/// from 10:00 answers between 12:00 and 13:00 the second scope alone, and a `scopeQ` /Madrid/Centro over that
/// window does not select it
/// Clause: 4.18, 5.7.3, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9034_a_later_scope_supersedes_the_one_before_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_superseded_id = namespace.urn("Vehicle", "superseded")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_superseded_evolution = TemporalEntity::builder()
        .id(vehicle_superseded_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2018-08-01T12:30:00Z".parse()?))
        .build()?;
    // The model refuses it (a temporal scope), so `scope` is written as text.
    let scope: RawJson = r#"[{"type": "Property", "value": "/Madrid/Centro", "observedAt": "2018-08-01T09:00:00Z"}, {"type": "Property", "value": "/Company123/UnitA", "observedAt": "2018-08-01T10:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(vehicle_superseded_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_superseded_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_superseded_id)
        .temporal_q(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.scope, len(eq(1)));
    expect_that!(
        evolution.scope.first().map(ScopeInstance::value),
        some(eq(&ScopeValue::Scopes(OneOrMany::one("/Company123/UnitA".parse::<Scope>()?))))
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .scope_q("/Madrid/Centro".parse::<ScopeQuery>()?)
        .ids(vec![vehicle_superseded_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
