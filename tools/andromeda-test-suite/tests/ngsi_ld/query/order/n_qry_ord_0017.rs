//! A date time ordering pages ties by identifier and entities lacking the attribute last.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{DateTime, Entity, EntityFragment, NgsiLdMediaType, Value},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy` on an attribute holding only `DateTime` values pages its instants ascending and descending with
/// ties in identifier order and the Entities without it last, and keeps that order once one Entity holds a string
/// Clause: 4.23.1, 4.23.2, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_ord_0017_a_date_time_ordering_pages_ties_by_identifier_and_entities_lacking_the_attribute_last() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let train_01_id = namespace.urn("Train", "01")?;
    let train_type = namespace.entity_type("Train")?;
    let train_02_id = namespace.urn("Train", "02")?;
    let train_03_id = namespace.urn("Train", "03")?;
    let train_04_id = namespace.urn("Train", "04")?;
    let train_05_id = namespace.urn("Train", "05")?;
    let train_06_id = namespace.urn("Train", "06")?;
    let train_07_id = namespace.urn("Train", "07")?;
    let train_08_id = namespace.urn("Train", "08")?;
    let train_09_id = namespace.urn("Train", "09")?;
    let train_10_id = namespace.urn("Train", 10)?;
    let train_11_id = namespace.urn("Train", 11)?;
    let train_12_id = namespace.urn("Train", 12)?;
    let entities = vec![
        Entity::builder()
            .id(train_01_id.clone())
            .entity_type(train_type.clone())
            .property(format!("departs{namespace}").as_str(), Value::from("2026-03-01T10:00:00Z".parse::<DateTime>()?))
            .build()?,
        Entity::builder()
            .id(train_02_id.clone())
            .entity_type(train_type.clone())
            .property(format!("departs{namespace}").as_str(), Value::from("2026-03-01T08:00:00Z".parse::<DateTime>()?))
            .build()?,
        Entity::builder()
            .id(train_03_id.clone())
            .entity_type(train_type.clone())
            .property(
                format!("departs{namespace}").as_str(),
                Value::from("2026-03-01T09:00:00.000Z".parse::<DateTime>()?),
            )
            .build()?,
        Entity::builder()
            .id(train_04_id.clone())
            .entity_type(train_type.clone())
            .property("unrelated", 1)
            .build()?,
        Entity::builder()
            .id(train_05_id.clone())
            .entity_type(train_type.clone())
            .property(format!("departs{namespace}").as_str(), Value::from("2026-03-01T08:00:00Z".parse::<DateTime>()?))
            .build()?,
        Entity::builder()
            .id(train_06_id.clone())
            .entity_type(train_type.clone())
            .property(
                format!("departs{namespace}").as_str(),
                Value::from("2026-03-01T12:30:00,5Z".parse::<DateTime>()?),
            )
            .build()?,
        Entity::builder()
            .id(train_07_id.clone())
            .entity_type(train_type.clone())
            .property(format!("departs{namespace}").as_str(), Value::from("2026-03-01T09:00:00Z".parse::<DateTime>()?))
            .build()?,
        Entity::builder()
            .id(train_08_id.clone())
            .entity_type(train_type.clone())
            .property(format!("departs{namespace}").as_str(), Value::from("2026-02-28T23:59:59Z".parse::<DateTime>()?))
            .build()?,
        Entity::builder()
            .id(train_09_id.clone())
            .entity_type(train_type.clone())
            .property(format!("departs{namespace}").as_str(), Value::from("2026-03-01T10:00:00Z".parse::<DateTime>()?))
            .build()?,
        Entity::builder()
            .id(train_10_id.clone())
            .entity_type(train_type.clone())
            .property("unrelated", 2)
            .build()?,
        Entity::builder()
            .id(train_11_id.clone())
            .entity_type(train_type.clone())
            .property(format!("departs{namespace}").as_str(), Value::from("2026-03-02T00:00:00Z".parse::<DateTime>()?))
            .build()?,
        Entity::builder()
            .id(train_12_id.clone())
            .entity_type(train_type.clone())
            .property(
                format!("departs{namespace}").as_str(),
                Value::from("2026-03-01T12:30:00.5Z".parse::<DateTime>()?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&train_type)
        .ordering(format!("departs{namespace}").as_str().parse::<OrderBy>()?)
        .limit(5)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(12)));
    expect_that!(page.items(), len(eq(5)));
    expect_that!(page.items(), nth(0, has_id(&train_08_id)));
    expect_that!(page.items(), nth(1, has_id(&train_02_id)));
    expect_that!(page.items(), nth(2, has_id(&train_05_id)));
    expect_that!(page.items(), nth(3, has_id(&train_03_id)));
    expect_that!(page.items(), nth(4, has_id(&train_07_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&train_type)
        .ordering(format!("departs{namespace}").as_str().parse::<OrderBy>()?)
        .limit(5)
        .offset(5)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(12)));
    expect_that!(page_2.items(), len(eq(5)));
    expect_that!(page_2.items(), nth(0, has_id(&train_01_id)));
    expect_that!(page_2.items(), nth(1, has_id(&train_09_id)));
    expect_that!(page_2.items(), nth(2, has_id(&train_06_id)));
    expect_that!(page_2.items(), nth(3, has_id(&train_12_id)));
    expect_that!(page_2.items(), nth(4, has_id(&train_11_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&train_type)
        .ordering(format!("departs{namespace}").as_str().parse::<OrderBy>()?)
        .limit(5)
        .offset(10)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(12)));
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&train_04_id)));
    expect_that!(page_3.items(), nth(1, has_id(&train_10_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&train_type)
        .ordering(format!("departs{namespace};desc").as_str().parse::<OrderBy>()?)
        .limit(12)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(12)));
    expect_that!(page_4.items(), nth(0, has_id(&train_11_id)));
    expect_that!(page_4.items(), nth(1, has_id(&train_06_id)));
    expect_that!(page_4.items(), nth(2, has_id(&train_12_id)));
    expect_that!(page_4.items(), nth(3, has_id(&train_01_id)));
    expect_that!(page_4.items(), nth(4, has_id(&train_09_id)));
    expect_that!(page_4.items(), nth(5, has_id(&train_03_id)));
    expect_that!(page_4.items(), nth(6, has_id(&train_07_id)));
    expect_that!(page_4.items(), nth(7, has_id(&train_02_id)));
    expect_that!(page_4.items(), nth(8, has_id(&train_05_id)));
    expect_that!(page_4.items(), nth(9, has_id(&train_08_id)));
    expect_that!(page_4.items(), nth(10, has_id(&train_04_id)));
    expect_that!(page_4.items(), nth(11, has_id(&train_10_id)));
    let fragment = EntityFragment::builder().property(format!("departs{namespace}").as_str(), "late").build()?;
    broker.entities().update_attributes(&train_09_id, &fragment).send().await?.done()?;
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&train_type)
        .ordering(format!("departs{namespace}").as_str().parse::<OrderBy>()?)
        .limit(12)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(12)));
    expect_that!(page_5.items(), nth(0, has_id(&train_09_id)));
    expect_that!(page_5.items(), nth(1, has_id(&train_08_id)));
    expect_that!(page_5.items(), nth(2, has_id(&train_02_id)));
    expect_that!(page_5.items(), nth(3, has_id(&train_05_id)));
    expect_that!(page_5.items(), nth(4, has_id(&train_03_id)));
    expect_that!(page_5.items(), nth(5, has_id(&train_07_id)));
    expect_that!(page_5.items(), nth(6, has_id(&train_01_id)));
    expect_that!(page_5.items(), nth(7, has_id(&train_06_id)));
    expect_that!(page_5.items(), nth(8, has_id(&train_12_id)));
    expect_that!(page_5.items(), nth(9, has_id(&train_11_id)));
    expect_that!(page_5.items(), nth(10, has_id(&train_04_id)));
    expect_that!(page_5.items(), nth(11, has_id(&train_10_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&train_type)
        .ordering(format!("departs{namespace};desc").as_str().parse::<OrderBy>()?)
        .limit(12)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(12)));
    expect_that!(page_6.items(), nth(0, has_id(&train_11_id)));
    expect_that!(page_6.items(), nth(1, has_id(&train_06_id)));
    expect_that!(page_6.items(), nth(2, has_id(&train_12_id)));
    expect_that!(page_6.items(), nth(3, has_id(&train_01_id)));
    expect_that!(page_6.items(), nth(4, has_id(&train_03_id)));
    expect_that!(page_6.items(), nth(5, has_id(&train_07_id)));
    expect_that!(page_6.items(), nth(6, has_id(&train_02_id)));
    expect_that!(page_6.items(), nth(7, has_id(&train_05_id)));
    expect_that!(page_6.items(), nth(8, has_id(&train_08_id)));
    expect_that!(page_6.items(), nth(9, has_id(&train_09_id)));
    expect_that!(page_6.items(), nth(10, has_id(&train_04_id)));
    expect_that!(page_6.items(), nth(11, has_id(&train_10_id)));
    Ok(())
}
