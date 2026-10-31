//! A purge removes the entities.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::Entity};

/// Pins: Purge Entities on broker A by type, over an Entity it holds and no registration, answers 204 and broker A
/// then finds no Entity of the type
/// Clause: 5.6.21.4, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_7600_a_purge_removes_the_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    broker.entities().purge().entity_type(&vehicle_type).send().await?.done()?;
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
