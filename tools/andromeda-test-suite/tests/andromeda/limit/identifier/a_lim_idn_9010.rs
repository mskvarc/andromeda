//! A new attribute name is accepted past eleven thousand names.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, NgsiLdMediaType},
    query::{Locality, q},
};

/// Entities the batch create spreads the filler attribute names over.
const FILL_ENTITIES: u32 = 11;

/// Distinct attribute names each filler Entity holds.
const NAMES_PER_ENTITY: u32 = 1000;

/// Pins: After Batch Entity Create of eleven Entities holding 11,000 distinct attribute names between them answers
/// 201, Create Entity with an attribute name never used answers 201 and reads it back, and Create Entity with an
/// established name answers 201 and q on that name selects both its Entities
/// Clause: 4.5.2, 5.6.1, 5.6.7, 4.9
/// Recorded: ETSI GS CIM 009 sets no bound on the number of attribute names or Entity types (clauses 4.5.1 and
/// 4.5.2); Andromeda bounds only the in-process cache of its name dictionaries, never the dictionaries
#[gtest]
#[tokio::test]
async fn a_lim_idn_9010_a_new_attribute_name_is_accepted_past_eleven_thousand_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_established_id = namespace.urn("Device", "established")?;
    let device_type = namespace.entity_type("Device")?;
    let device_novel_id = namespace.urn("Device", "novel")?;
    let device_repeat_id = namespace.urn("Device", "repeat")?;
    let device_established = Entity::builder()
        .id(device_established_id.clone())
        .entity_type(device_type.clone())
        .property(format!("established{namespace}").as_str(), 1)
        .build()?;
    broker.entities().create(&device_established).send().await?.done()?;
    let entities = (0..FILL_ENTITIES)
        .map(|index| -> Result<Entity<Input>> {
            let first = index * NAMES_PER_ENTITY;
            let builder = Entity::builder()
                .id(namespace.urn("Device", format!("fill-{index}"))?)
                .entity_type(device_type.clone());
            Ok((first..first + NAMES_PER_ENTITY)
                .fold(builder, |builder, name| builder.property(format!("fill{name:05}").as_str(), 1))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let device_novel = Entity::builder()
        .id(device_novel_id.clone())
        .entity_type(device_type.clone())
        .property(format!("novel{namespace}").as_str(), 1)
        .build()?;
    broker.entities().create(&device_novel).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_novel_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute(&format!("novel{namespace}"), any_attribute().value(1)));
    let device_repeat = Entity::builder()
        .id(device_repeat_id.clone())
        .entity_type(device_type.clone())
        .property(format!("established{namespace}").as_str(), 1)
        .build()?;
    broker.entities().create(&device_repeat).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .locality(Locality::Local)
        .q(q::attr(format!("established{namespace}").as_str()).eq(1))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&device_established_id)));
    expect_that!(page.items(), contains(has_id(&device_repeat_id)));
    Ok(())
}
