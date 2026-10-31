//! Deleting an instance current state lacks keeps the entity modified at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    deleted_instance::deleted_instance,
    entity_matcher::{has_attribute, lacks_attribute},
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    system_time::{SystemMember, of_attribute, of_entity},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{
        AttributeKind,
        Entity,
        NgsiLdMediaType,
        Property,
        evolution::{DeletedInstance, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Add Attributes to Temporal Evolution recording the deletion of a humidity instance (urn:ngsi-ld:null with
/// `deletedAt`) on a Vehicle whose current state holds no humidity answers 204, records the deletion in the
/// Temporal Evolution and leaves the entity-level and the speed `modifiedAt` as they were
/// Clause: 4.5.7, 4.8, 5.8.6, 5.6.12.4
/// Recorded: clause 5.6.12.4 adds every instance to the Temporal Evolution and says nothing of current state;
/// Andromeda folds the instances into current state by the clause 4.5.5.3 rule, and a deletion of an instance
/// current state does not hold modifies nothing there, so by clause 4.8 no `modifiedAt` moves
#[gtest]
#[tokio::test]
async fn o_tmp_add_9009_deleting_an_instance_current_state_lacks_keeps_the_entity_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "absent-deletion")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let before = json
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_modified = *of_entity(before.body(), SystemMember::ModifiedAt)?;
    let speed_modified = *of_attribute(before.body(), &["speed"], SystemMember::ModifiedAt)?;
    let humidity_deletion = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .deleted(
            "humidity",
            DeletedInstance::builder(AttributeKind::Property, "2026-10-19T11:00:00Z".parse()?)
                .observed_at("2026-10-19T11:00:00Z".parse()?)
                .build(),
        )
        .build()?
        .attributes;
    let appended = broker.with_responses().temporal().add_attributes(&vehicle_id, &humidity_deletion).send().await;
    expect_that!(&appended, answered(StatusCode::NO_CONTENT));
    let after = json
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(of_entity(after.body(), SystemMember::ModifiedAt)?, eq(&entity_modified));
    expect_that!(after.body(), lacks_attribute("humidity"));
    expect_that!(after.body(), has_attribute("speed", any_attribute().value(40)));
    expect_that!(of_attribute(after.body(), &["speed"], SystemMember::ModifiedAt)?, eq(&speed_modified));
    let evolution = json
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(evolution.body(), has_evolution("humidity", contains(deleted_instance())));
    Ok(())
}
