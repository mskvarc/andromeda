//! A relationship equality matches any of several objects.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: a q equality on a Relationship holding several objects matches the entity when the operand is any one of
/// them, the last included, and not when it is none of them
/// Clause: 4.9, 4.5.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0035_a_relationship_equality_matches_any_of_several_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let machine_id = namespace.urn("Machine", 1)?;
    let machine_type = namespace.entity_type("Machine")?;
    let part_a_id = namespace.urn("Part", "a")?;
    let part_b_id = namespace.urn("Part", "b")?;
    let part_c_id = namespace.urn("Part", "c")?;
    let machine_2_id = namespace.urn("Machine", 2)?;
    let entities = [
        Entity::builder()
            .id(machine_id.clone())
            .entity_type(machine_type.clone())
            .relationship("hasPart", vec![part_a_id.clone(), part_b_id.clone(), part_c_id])
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
        .q(q::attr("hasPart").eq(format!("urn:ngsi-ld:Part:{namespace}-c").as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&machine_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&machine_type)
        .q(q::attr("hasPart").eq(format!("urn:ngsi-ld:Part:{namespace}-b").as_str()))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    Ok(())
}
