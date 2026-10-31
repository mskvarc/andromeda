//! The type selection follows a type change and a deletion.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
};
use std::iter;

/// Pins: Query Entities of the late type, after Replace Entity drops it from the Entity carrying both, answers the
/// five others in identifier order counted 5; after a replacement adding it back, the six; and of the early type,
/// after Delete Entity of that Entity, the six others counted 6
/// Clause: 4.17, 5.6.18, 5.6.6, 5.5.9
/// Recorded: clause 5.5.9 leaves the order of the pages to the implementation; Andromeda pages a query that asks
/// for no ordering in identifier order
#[gtest]
#[tokio::test]
async fn a_qry_typ_9005_the_type_selection_follows_a_type_change_and_a_deletion() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let a_0_id = namespace.urn("A", "0")?;
    let membership_early_type = namespace.entity_type("MembershipEarly")?;
    let m_0_id = namespace.urn("M", "0")?;
    let z_0_id = namespace.urn("Z", "0")?;
    let membership_late_type = namespace.entity_type("MembershipLate")?;
    let z_id = namespace.urn("Z", 1)?;
    let z_2_id = namespace.urn("Z", 2)?;
    let z_3_id = namespace.urn("Z", 3)?;
    let z_4_id = namespace.urn("Z", 4)?;
    let a_id = namespace.urn("A", 1)?;
    let a_2_id = namespace.urn("A", 2)?;
    let a_3_id = namespace.urn("A", 3)?;
    let a_4_id = namespace.urn("A", 4)?;
    let a_5_id = namespace.urn("A", 5)?;
    let entities = (0..=5)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("A", n)?)
                .entity_type(membership_early_type.clone())
                .property("name", format!("urn:ngsi-ld:A:{namespace}-{n}"))
                .build()?)
        })
        .chain(iter::once(Ok(Entity::builder()
            .id(m_0_id.clone())
            .entity_type(vec![membership_early_type.clone(), namespace.entity_type("MembershipLate")?])
            .property("name", format!("urn:ngsi-ld:M:{namespace}-0"))
            .build()?)))
        .chain((0..=4).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Z", n)?)
                .entity_type(membership_late_type.clone())
                .property("name", format!("urn:ngsi-ld:Z:{namespace}-{n}"))
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let m_0_replacement = Entity::builder()
        .id(m_0_id.clone())
        .entity_type(membership_early_type.clone())
        .property("name", "moved")
        .build()?;
    broker.entities().replace(&m_0_replacement).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_late_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(0)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(5)));
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&z_0_id)));
    expect_that!(page.items(), nth(1, has_id(&z_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_late_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(5)));
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&z_2_id)));
    expect_that!(page_2.items(), nth(1, has_id(&z_3_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_late_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(4)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(5)));
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&z_4_id)));
    let m_0_replacement_2 = Entity::builder()
        .id(m_0_id.clone())
        .entity_type(vec![membership_late_type.clone(), membership_early_type.clone()])
        .property("name", "moved")
        .build()?;
    broker.entities().replace(&m_0_replacement_2).send().await?.done()?;
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_late_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(0)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(6)));
    expect_that!(page_4.items(), len(eq(2)));
    expect_that!(page_4.items(), nth(0, has_id(&m_0_id)));
    expect_that!(page_4.items(), nth(1, has_id(&z_0_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_late_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(6)));
    expect_that!(page_5.items(), len(eq(2)));
    expect_that!(page_5.items(), nth(0, has_id(&z_id)));
    expect_that!(page_5.items(), nth(1, has_id(&z_2_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_late_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(4)
        .send()
        .await?;
    expect_that!(page_6.total(), some(eq(6)));
    expect_that!(page_6.items(), len(eq(2)));
    expect_that!(page_6.items(), nth(0, has_id(&z_3_id)));
    expect_that!(page_6.items(), nth(1, has_id(&z_4_id)));
    broker.entities().delete(&m_0_id).send().await?.done()?;
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_early_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(0)
        .send()
        .await?;
    expect_that!(page_7.total(), some(eq(6)));
    expect_that!(page_7.items(), len(eq(2)));
    expect_that!(page_7.items(), nth(0, has_id(&a_0_id)));
    expect_that!(page_7.items(), nth(1, has_id(&a_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_early_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_8.total(), some(eq(6)));
    expect_that!(page_8.items(), len(eq(2)));
    expect_that!(page_8.items(), nth(0, has_id(&a_2_id)));
    expect_that!(page_8.items(), nth(1, has_id(&a_3_id)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_early_type)
        .count(ResultsCount::Include)
        .limit(2)
        .offset(4)
        .send()
        .await?;
    expect_that!(page_9.total(), some(eq(6)));
    expect_that!(page_9.items(), len(eq(2)));
    expect_that!(page_9.items(), nth(0, has_id(&a_4_id)));
    expect_that!(page_9.items(), nth(1, has_id(&a_5_id)));
    Ok(())
}
