//! A late sorting type pages in identifier order.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
    query::Locality,
};
use std::iter;

/// Pins: Query Entities of the type whose identifiers sort after another type's answers its six Entities, the one
/// carrying both types first, two per page in identifier order by offset with NGSILD-Results-Count 6 on every page,
/// and the same by the next links of local=true
/// Clause: 4.17, 5.5.9, 6.3.13
/// Recorded: clause 5.5.9 leaves the order of the pages to the implementation; Andromeda pages a query that asks
/// for no ordering in identifier order
#[gtest]
#[tokio::test]
async fn a_qry_typ_9002_a_late_sorting_type_pages_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let membership_early_type = namespace.entity_type("MembershipEarly")?;
    let m_0_id = namespace.urn("M", "0")?;
    let z_0_id = namespace.urn("Z", "0")?;
    let membership_late_type = namespace.entity_type("MembershipLate")?;
    let z_id = namespace.urn("Z", 1)?;
    let z_2_id = namespace.urn("Z", 2)?;
    let z_3_id = namespace.urn("Z", 3)?;
    let z_4_id = namespace.urn("Z", 4)?;
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
    expect_that!(page.total(), some(eq(6)));
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&m_0_id)));
    expect_that!(page.items(), nth(1, has_id(&z_0_id)));
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
    expect_that!(page_2.total(), some(eq(6)));
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&z_id)));
    expect_that!(page_2.items(), nth(1, has_id(&z_2_id)));
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
    expect_that!(page_3.total(), some(eq(6)));
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&z_3_id)));
    expect_that!(page_3.items(), nth(1, has_id(&z_4_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&membership_late_type)
        .limit(2)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(2)));
    expect_that!(page_4.items(), nth(0, has_id(&m_0_id)));
    expect_that!(page_4.items(), nth(1, has_id(&z_0_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page_4.next_page().or_fail()?)
        .await?;
    expect_that!(page_5.items(), len(eq(2)));
    expect_that!(page_5.items(), nth(0, has_id(&z_id)));
    expect_that!(page_5.items(), nth(1, has_id(&z_2_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page_5.next_page().or_fail()?)
        .await?;
    expect_that!(page_6.items(), len(eq(2)));
    expect_that!(page_6.items(), nth(0, has_id(&z_3_id)));
    expect_that!(page_6.items(), nth(1, has_id(&z_4_id)));
    Ok(())
}
