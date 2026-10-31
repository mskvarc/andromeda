//! A deleted entity is selected by type in the temporal api alone.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: after Delete Entity of an Entity of type Vehicle created with speed 30 observed at 2021-01-01, Query
/// Entities with type=Vehicle answers no Entity, while Query Temporal Evolution of Entities with the same type and
/// timerel=after 2020-01-01 answers it with its speed instance
/// Clause: 5.6.6, 5.7.2, 5.7.4, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default and keeps the Temporal Evolution of a deleted
/// Entity, so the Entity's type selects it in the Temporal API alone
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9078_a_deleted_entity_is_selected_by_type_in_the_temporal_api_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_typed_deleted_id = namespace.urn("Vehicle", "typed-deleted")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_typed_deleted = Entity::builder()
        .id(vehicle_typed_deleted_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_typed_deleted).send().await?.done()?;
    broker.entities().delete(&vehicle_typed_deleted_id).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(
        page_2.items(),
        nth(
            0,
            all!(has_id(&vehicle_typed_deleted_id), has_evolution("speed", nth(0, any_attribute().value(30))))
        )
    );
    Ok(())
}
