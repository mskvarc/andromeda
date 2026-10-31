//! A ld json fragment holding only the context changes nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_context,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::SystemAttributes},
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::temporal_query::TemporalProperty,
};

/// Pins: Merge Entity with an application/ld+json Fragment holding only the @context answers 204 and changes
/// nothing: no value, no `modifiedAt`, no new temporal instance
/// Clause: 5.4, 5.5.12, 5.6.17.4, 4.8
/// Recorded: a Fragment is an IETF RFC 7396 Merge Patch, so an empty Fragment modifies nothing
#[gtest]
#[tokio::test]
async fn n_ent_mrg_2632_a_ld_json_fragment_holding_only_the_context_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_modified = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    let fragment = EntityFragment::builder().build()?;
    core_context_body.entities().merge(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::ModifiedAt)?, eq(entity_modified));
    expect_that!(system_time::of_attribute(entity_2, &["speed"], SystemMember::ModifiedAt)?, eq(speed_modified));
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(55)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    Ok(())
}
