//! The reified scope carries its temporal properties.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
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

/// Pins: Retrieve Temporal Evolution of an Entity with sysAttrs answers one scope instance, a Property holding
/// /Madrid/Centro observed at 11:00 with a createdAt and a modifiedAt
/// Clause: 4.18, 4.5.6
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9030_the_reified_scope_carries_its_temporal_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_reified_id = namespace.urn("Vehicle", "reified")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let eleven = "2018-08-01T11:00:00Z".parse::<DateTime>()?;
    let vehicle_reified_evolution = TemporalEntity::builder()
        .id(vehicle_reified_id.clone())
        .entity_type(vehicle_type)
        .scope(ScopeInstance::new("/Madrid/Centro".parse::<Scope>()?).with_observed_at(eleven))
        .property("speed", Property::builder(30).observed_at("2018-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_reified_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_reified_id)
        .temporal_q(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(&evolution.scope, len(eq(1)));
    let instance = evolution.scope.first().or_fail()?;
    expect_that!(instance.value(), eq(&ScopeValue::Scopes(OneOrMany::one("/Madrid/Centro".parse::<Scope>()?))));
    expect_that!(instance.observed_at().map(DateTime::timestamp), some(eq(eleven.timestamp())));
    expect_that!(instance.output().created_at, some(anything()));
    expect_that!(instance.output().modified_at, some(anything()));
    Ok(())
}
