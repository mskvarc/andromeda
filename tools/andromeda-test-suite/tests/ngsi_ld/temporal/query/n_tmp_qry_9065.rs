//! Attrs alone pages the holders that sort last in identifier order.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    identity_matcher::{has_id, ids_in_order},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, temporal_query},
};
use std::iter;

/// Pins: Query Temporal Evolution of Entities by attrs alone over 300 evolutions of which only the last 20 in
/// identifier order hold the queried attribute pages exactly those 20 in identifier order, ten at a time, with a
/// total of 20, by GET and by POST query; an evolution holding the attribute under two `datasetIds` is listed once,
/// two attributes list the holders of either once each, and a window none of the holders answers lists nothing
/// Clause: 5.7.4.4, 5.5.9, 4.5.5
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9065_attrs_alone_pages_the_holders_that_sort_last_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let holder_type = namespace.entity_type("Holder")?;
    let attr_walk_0281_id = namespace.urn("AttrWalk", "0281")?;
    let attr_walk_0285_id = namespace.urn("AttrWalk", "0285")?;
    let attr_walk_0286_id = namespace.urn("AttrWalk", "0286")?;
    let attr_walk_0282_id = namespace.urn("AttrWalk", "0282")?;
    let attr_walk_0283_id = namespace.urn("AttrWalk", "0283")?;
    let attr_walk_0284_id = namespace.urn("AttrWalk", "0284")?;
    let attr_walk_0287_id = namespace.urn("AttrWalk", "0287")?;
    let attr_walk_0288_id = namespace.urn("AttrWalk", "0288")?;
    let attr_walk_0289_id = namespace.urn("AttrWalk", "0289")?;
    let attr_walk_0290_id = namespace.urn("AttrWalk", "0290")?;
    let attr_walk_0279_id = namespace.urn("AttrWalk", "0279")?;
    let attr_walk_0280_id = namespace.urn("AttrWalk", "0280")?;
    let evolutions = (1..=280)
        .map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("AttrWalk", format!("{n:04}"))?)
                .entity_type(holder_type.clone())
                .property(namespace.attribute("other")?, Property::builder(n).observed_at("2026-03-01T10:00:00Z".parse()?))
                .build()?)
        })
        .chain((281..=284).map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("AttrWalk", format!("{n:04}"))?)
                .entity_type(holder_type.clone())
                .property(namespace.attribute("held")?, Property::builder(n).observed_at("2026-03-01T10:00:00Z".parse()?))
                .build()?)
        }))
        .chain(iter::once(Ok(TemporalEntity::builder()
            .id(attr_walk_0285_id.clone())
            .entity_type(holder_type.clone())
            .property(
                namespace.attribute("held")?,
                Property::builder(285)
                    .dataset_id(namespace.dataset_id("Dataset", "a")?)
                    .observed_at("2026-03-01T10:00:00Z".parse()?),
            )
            .property(
                namespace.attribute("held")?,
                Property::builder(285)
                    .dataset_id(namespace.dataset_id("Dataset", "b")?)
                    .observed_at("2026-03-01T10:00:00Z".parse()?),
            )
            .build()?)))
        .chain((286..=300).map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("AttrWalk", format!("{n:04}"))?)
                .entity_type(holder_type.clone())
                .property(namespace.attribute("held")?, Property::builder(n).observed_at("2026-03-01T10:00:00Z".parse()?))
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    for evolution in &evolutions {
        broker.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .attrs(vec![format!("held_{namespace}").parse()?])
        .limit(10)
        .offset(0)
        .locality(Locality::Local)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(20)));
    expect_that!(page.items(), len(eq(10)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&attr_walk_0281_id),
                has_evolution(&format!("held_{namespace}"), anything()),
                lacks_evolution(&format!("other_{namespace}"))
            )
        )
    );
    expect_that!(page.items(), nth(1, has_id(&attr_walk_0282_id)));
    expect_that!(page.items(), nth(2, has_id(&attr_walk_0283_id)));
    expect_that!(page.items(), nth(3, has_id(&attr_walk_0284_id)));
    expect_that!(
        page.items(),
        nth(4, all!(has_id(&attr_walk_0285_id), has_evolution(&format!("held_{namespace}"), len(eq(2)))))
    );
    expect_that!(page.items(), nth(5, has_id(&attr_walk_0286_id)));
    expect_that!(page.items(), nth(6, has_id(&attr_walk_0287_id)));
    expect_that!(page.items(), nth(7, has_id(&attr_walk_0288_id)));
    expect_that!(page.items(), nth(8, has_id(&attr_walk_0289_id)));
    expect_that!(page.items(), nth(9, has_id(&attr_walk_0290_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .attrs(vec![format!("held_{namespace}").parse()?])
        .limit(10)
        .offset(10)
        .locality(Locality::Local)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(20)));
    let expected_ids = (291..=300)
        .map(|n| namespace.urn("AttrWalk", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_2.items(), ids_in_order(&expected_ids));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .attrs(vec![format!("held_{namespace}").parse()?])
        .limit(10)
        .offset(20)
        .locality(Locality::Local)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(20)));
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .attrs(vec![format!("held_{namespace}").parse()?])
        .locality(Locality::Local)
        .limit(10)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(20)));
    let expected_ids_2 = (281..=290)
        .map(|n| namespace.urn("AttrWalk", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_4.items(), ids_in_order(&expected_ids_2));
    // both attributes list every evolution once, the holders of either in identifier order
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .attrs(vec![format!("held_{namespace}").parse()?, format!("other_{namespace}").parse()?])
        .limit(3)
        .offset(278)
        .locality(Locality::Local)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(300)));
    expect_that!(page_5.items(), len(eq(3)));
    expect_that!(page_5.items(), nth(0, has_id(&attr_walk_0279_id)));
    expect_that!(page_5.items(), nth(1, has_id(&attr_walk_0280_id)));
    expect_that!(page_5.items(), nth(2, has_id(&attr_walk_0281_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::before("2026-01-01T00:00:00Z".parse()?))
        .attrs(vec![format!("held_{namespace}").parse()?])
        .limit(10)
        .offset(0)
        .locality(Locality::Local)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_6.total(), some(eq(0)));
    expect_that!(page_6.items(), len(eq(0)));
    Ok(())
}
