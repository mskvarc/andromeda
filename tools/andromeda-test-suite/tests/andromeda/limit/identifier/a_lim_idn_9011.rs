//! A new entity type is accepted past eleven thousand types.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityTypeName, Input, NgsiLdMediaType},
    query::Locality,
};

/// Entities the batch create spreads the filler Entity types over.
const FILL_ENTITIES: u32 = 11;

/// Distinct Entity types each filler Entity holds.
const TYPES_PER_ENTITY: u32 = 1000;

/// Pins: After Batch Entity Create of eleven Entities holding 11,000 distinct Entity types between them answers
/// 201, Create Entity of a type never used answers 201 and Query Entities by it answers that Entity, and Create
/// Entity of an established type answers 201 and Query Entities by it answers both its Entities
/// Clause: 4.5.1, 5.6.1, 5.6.7, 5.7.2
/// Recorded: ETSI GS CIM 009 sets no bound on the number of attribute names or Entity types (clauses 4.5.1 and
/// 4.5.2); Andromeda bounds only the in-process cache of its name dictionaries, never the dictionaries
#[gtest]
#[tokio::test]
async fn a_lim_idn_9011_a_new_entity_type_is_accepted_past_eleven_thousand_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_established_type_id = namespace.urn("Device", "established-type")?;
    let device_novel_type_id = namespace.urn("Device", "novel-type")?;
    let device_repeat_type_id = namespace.urn("Device", "repeat-type")?;
    let device_established_type = Entity::builder()
        .id(device_established_type_id.clone())
        .entity_type(format!("Established{namespace}").parse::<EntityTypeName>()?)
        .build()?;
    broker.entities().create(&device_established_type).send().await?.done()?;
    let entities = (0..FILL_ENTITIES)
        .map(|index| -> Result<Entity<Input>> {
            let first = index * TYPES_PER_ENTITY;
            let types = (first..first + TYPES_PER_ENTITY)
                .map(|name| format!("FillType{name:05}").parse::<EntityTypeName>())
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Entity::builder()
                .id(namespace.urn("Device", format!("type-fill-{index}"))?)
                .entity_type(types)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let device_novel_type = Entity::builder()
        .id(device_novel_type_id.clone())
        .entity_type(format!("Novel{namespace}").parse::<EntityTypeName>()?)
        .build()?;
    broker.entities().create(&device_novel_type).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(format!("Novel{namespace}").as_str())
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&device_novel_type_id)));
    let device_repeat_type = Entity::builder()
        .id(device_repeat_type_id.clone())
        .entity_type(format!("Established{namespace}").parse::<EntityTypeName>()?)
        .build()?;
    broker.entities().create(&device_repeat_type).send().await?.done()?;
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(format!("Established{namespace}").as_str())
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&device_established_type_id)));
    expect_that!(page_2.items(), contains(has_id(&device_repeat_type_id)));
    Ok(())
}
