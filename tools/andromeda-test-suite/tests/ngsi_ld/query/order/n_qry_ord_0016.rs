//! A number ordering pages ties by identifier and entities lacking the attribute last.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, EntityFragment, NgsiLdMediaType, Value},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy` on an attribute holding only numbers pages its values ascending and descending with ties in
/// identifier order and the Entities without it last, and keeps that order once one Entity holds a string
/// Clause: 4.23.1, 4.23.2, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_ord_0016_a_number_ordering_pages_ties_by_identifier_and_entities_lacking_the_attribute_last() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let lap_01_id = namespace.urn("Lap", "01")?;
    let lap_type = namespace.entity_type("Lap")?;
    let lap_02_id = namespace.urn("Lap", "02")?;
    let lap_03_id = namespace.urn("Lap", "03")?;
    let lap_04_id = namespace.urn("Lap", "04")?;
    let lap_05_id = namespace.urn("Lap", "05")?;
    let lap_06_id = namespace.urn("Lap", "06")?;
    let lap_07_id = namespace.urn("Lap", "07")?;
    let lap_08_id = namespace.urn("Lap", "08")?;
    let lap_09_id = namespace.urn("Lap", "09")?;
    let lap_10_id = namespace.urn("Lap", 10)?;
    let lap_11_id = namespace.urn("Lap", 11)?;
    let lap_12_id = namespace.urn("Lap", 12)?;
    let entities = vec![
        Entity::builder()
            .id(lap_01_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), 3)
            .build()?,
        Entity::builder()
            .id(lap_02_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), 1)
            .build()?,
        Entity::builder()
            .id(lap_03_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), 2)
            .build()?,
        Entity::builder()
            .id(lap_04_id.clone())
            .entity_type(lap_type.clone())
            .property("unrelated", 1)
            .build()?,
        Entity::builder()
            .id(lap_05_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), 1)
            .build()?,
        Entity::builder()
            .id(lap_06_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), Value::Number("3.5".parse()?))
            .build()?,
        Entity::builder()
            .id(lap_07_id.clone())
            .entity_type(lap_type.clone())
            .property("unrelated", 1)
            .build()?,
        Entity::builder()
            .id(lap_08_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), 2)
            .build()?,
        Entity::builder()
            .id(lap_09_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), -4)
            .build()?,
        Entity::builder()
            .id(lap_10_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), 1)
            .build()?,
        Entity::builder()
            .id(lap_11_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), 10)
            .build()?,
        Entity::builder()
            .id(lap_12_id.clone())
            .entity_type(lap_type.clone())
            .property(format!("rank{namespace}").as_str(), 2)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&lap_type)
        .ordering(format!("rank{namespace}").as_str().parse::<OrderBy>()?)
        .limit(5)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(12)));
    expect_that!(page.items(), len(eq(5)));
    expect_that!(page.items(), nth(0, has_id(&lap_09_id)));
    expect_that!(page.items(), nth(1, has_id(&lap_02_id)));
    expect_that!(page.items(), nth(2, has_id(&lap_05_id)));
    expect_that!(page.items(), nth(3, has_id(&lap_10_id)));
    expect_that!(page.items(), nth(4, has_id(&lap_03_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&lap_type)
        .ordering(format!("rank{namespace}").as_str().parse::<OrderBy>()?)
        .limit(5)
        .offset(5)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(5)));
    expect_that!(page_2.items(), nth(0, has_id(&lap_08_id)));
    expect_that!(page_2.items(), nth(1, has_id(&lap_12_id)));
    expect_that!(page_2.items(), nth(2, has_id(&lap_01_id)));
    expect_that!(page_2.items(), nth(3, has_id(&lap_06_id)));
    expect_that!(page_2.items(), nth(4, has_id(&lap_11_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&lap_type)
        .ordering(format!("rank{namespace}").as_str().parse::<OrderBy>()?)
        .limit(5)
        .offset(10)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&lap_04_id)));
    expect_that!(page_3.items(), nth(1, has_id(&lap_07_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&lap_type)
        .ordering(format!("rank{namespace};desc").as_str().parse::<OrderBy>()?)
        .limit(12)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(12)));
    expect_that!(page_4.items(), nth(0, has_id(&lap_11_id)));
    expect_that!(page_4.items(), nth(1, has_id(&lap_06_id)));
    expect_that!(page_4.items(), nth(2, has_id(&lap_01_id)));
    expect_that!(page_4.items(), nth(3, has_id(&lap_03_id)));
    expect_that!(page_4.items(), nth(4, has_id(&lap_08_id)));
    expect_that!(page_4.items(), nth(5, has_id(&lap_12_id)));
    expect_that!(page_4.items(), nth(6, has_id(&lap_02_id)));
    expect_that!(page_4.items(), nth(7, has_id(&lap_05_id)));
    expect_that!(page_4.items(), nth(8, has_id(&lap_10_id)));
    expect_that!(page_4.items(), nth(9, has_id(&lap_09_id)));
    expect_that!(page_4.items(), nth(10, has_id(&lap_04_id)));
    expect_that!(page_4.items(), nth(11, has_id(&lap_07_id)));
    let fragment = EntityFragment::builder().property(format!("rank{namespace}").as_str(), "unranked").build()?;
    broker.entities().append_attributes(&lap_07_id, &fragment).send().await?.done()?;
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&lap_type)
        .ordering(format!("rank{namespace}").as_str().parse::<OrderBy>()?)
        .limit(12)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(12)));
    expect_that!(page_5.items(), nth(0, has_id(&lap_09_id)));
    expect_that!(page_5.items(), nth(1, has_id(&lap_02_id)));
    expect_that!(page_5.items(), nth(2, has_id(&lap_05_id)));
    expect_that!(page_5.items(), nth(3, has_id(&lap_10_id)));
    expect_that!(page_5.items(), nth(4, has_id(&lap_03_id)));
    expect_that!(page_5.items(), nth(5, has_id(&lap_08_id)));
    expect_that!(page_5.items(), nth(6, has_id(&lap_12_id)));
    expect_that!(page_5.items(), nth(7, has_id(&lap_01_id)));
    expect_that!(page_5.items(), nth(8, has_id(&lap_06_id)));
    expect_that!(page_5.items(), nth(9, has_id(&lap_11_id)));
    expect_that!(page_5.items(), nth(10, has_id(&lap_07_id)));
    expect_that!(page_5.items(), nth(11, has_id(&lap_04_id)));
    Ok(())
}
