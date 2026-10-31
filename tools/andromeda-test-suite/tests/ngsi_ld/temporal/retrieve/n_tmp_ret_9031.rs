//! The reified scope collapses into temporal values.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        DateTime,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        Scope,
        evolution::{ScopeInstance, ScopeValue, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity with temporalValues answers scope as a Property whose values hold
/// /Madrid/Centro paired with 11:00
/// Clause: 4.18, 4.5.6, 4.5.9
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9031_the_reified_scope_collapses_into_temporal_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_reified_values_id = namespace.urn("Vehicle", "reified-values")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let eleven = "2018-08-01T11:00:00Z".parse::<DateTime>()?;
    let vehicle_reified_values_evolution = TemporalEntity::builder()
        .id(vehicle_reified_values_id.clone())
        .entity_type(vehicle_type)
        .scope(ScopeInstance::new("/Madrid/Centro".parse::<Scope>()?).with_observed_at(eleven))
        .property("speed", Property::builder(30).observed_at("2018-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_reified_values_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&vehicle_reified_values_id)
        .temporal_q(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let scope = &retrieved.body().scope;
    expect_that!(scope, len(eq(1)));
    let pair = scope.first().or_fail()?;
    expect_that!(&pair.value, eq(&ScopeValue::Scopes(OneOrMany::one("/Madrid/Centro".parse::<Scope>()?))));
    expect_that!(pair.time.timestamp(), eq(eleven.timestamp()));
    Ok(())
}
