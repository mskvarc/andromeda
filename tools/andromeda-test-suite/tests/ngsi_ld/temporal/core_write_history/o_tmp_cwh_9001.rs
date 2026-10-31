//! A replacement omitting the scope closes its interval.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, evolution::ScopeValue},
};

/// Pins: Replace Entity omitting the scope of an Entity created with scope /Madrid/Centro answers 204, and the
/// Temporal Evolution records the removal of the scope as the NGSI-LD Null with a `deletedAt`
/// Clause: 4.18, 5.6.18.4, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9001_a_replacement_omitting_the_scope_closes_its_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_replace_drops_id = namespace.urn("Vehicle", "replace-drops")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_replace_drops = Entity::builder()
        .id(vehicle_replace_drops_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid/Centro")
        .property("speed", 30)
        .build()?;
    broker.entities().create(&vehicle_replace_drops).send().await?.done()?;
    let vehicle_replace_drops_replacement = Entity::builder()
        .id(vehicle_replace_drops_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .build()?;
    broker.entities().replace(&vehicle_replace_drops_replacement).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_replace_drops_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution
            .scope
            .iter()
            .find(|instance| instance.value() == &ScopeValue::Removed)
            .and_then(|instance| instance.output().deleted_at),
        some(anything())
    );
    Ok(())
}
