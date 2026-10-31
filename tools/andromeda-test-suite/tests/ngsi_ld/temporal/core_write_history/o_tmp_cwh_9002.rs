//! Deleting the entity closes its scope interval.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, evolution::ScopeValue},
};

/// Pins: Delete Entity of an Entity created with scope /Madrid/Centro answers 204, and the Temporal Evolution
/// records the removal of the scope as the NGSI-LD Null with a `deletedAt`
/// Clause: 4.18, 5.6.6, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9002_deleting_the_entity_closes_its_scope_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_deleted_scope_id = namespace.urn("Vehicle", "deleted-scope")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_deleted_scope = Entity::builder()
        .id(vehicle_deleted_scope_id.clone())
        .entity_type(vehicle_type)
        .scope("/Madrid/Centro")
        .property("speed", 30)
        .build()?;
    broker.entities().create(&vehicle_deleted_scope).send().await?.done()?;
    broker.entities().delete(&vehicle_deleted_scope_id).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_deleted_scope_id)
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
