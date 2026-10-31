//! A hop follows every object of a relationship.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: a q hop over a Relationship holding several objects selects the root when the entity any one object names
/// satisfies the term, the last object included, and not when none does
/// Clause: 4.9, 4.5.3, 4.5.23, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0012_a_hop_follows_every_object_of_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let part_a_id = namespace.urn("Part", "a")?;
    let part_type = namespace.entity_type("Part")?;
    let part_b_id = namespace.urn("Part", "b")?;
    let part_c_id = namespace.urn("Part", "c")?;
    let machine_id = namespace.urn("Machine", 1)?;
    let machine_type = namespace.entity_type("Machine")?;
    let machine_2_id = namespace.urn("Machine", 2)?;
    let entities = [
        Entity::builder()
            .id(part_a_id.clone())
            .entity_type(part_type.clone())
            .property("name", "A")
            .build()?,
        Entity::builder()
            .id(part_b_id.clone())
            .entity_type(part_type.clone())
            .property("name", "B")
            .build()?,
        Entity::builder()
            .id(part_c_id.clone())
            .entity_type(part_type.clone())
            .property("name", "C")
            .build()?,
        Entity::builder()
            .id(machine_id.clone())
            .entity_type(machine_type.clone())
            .relationship("hasPart", vec![part_a_id.clone(), part_b_id.clone(), part_c_id.clone()])
            .build()?,
        Entity::builder()
            .id(machine_2_id)
            .entity_type(machine_type.clone())
            .relationship("hasPart", vec![part_a_id.clone(), part_b_id.clone()])
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&machine_type)
        .q(q::attr("hasPart").linked("name").eq("C"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&machine_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&machine_type)
        .q(q::attr("hasPart").linked("name").eq("A"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    Ok(())
}
