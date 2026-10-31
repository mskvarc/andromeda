//! An entity the dataset step empties is counted and paged.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_lookup::with_id,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{NgsiLdMediaType, OneOrMany, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with the dataset of one Entity's fuel, over that Entity and one
/// holding speed alone, answers both, the first with fuel alone and the second with no Attribute, and two pages of
/// one Entity each, both counted 2
/// Clause: 5.7.4.4, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9044_an_entity_the_dataset_step_empties_is_counted_and_paged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let a_id = namespace.urn("TqdsPaged", "a")?;
    let b_id = namespace.urn("TqdsPaged", "b")?;
    let tqds_paged_type = namespace.entity_type("TqdsPaged")?;
    let named = namespace.dataset_id("Dataset", "named")?;
    let a_evolution = TemporalEntity::builder()
        .id(a_id.clone())
        .entity_type(tqds_paged_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property(
            "fuel",
            Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?).dataset_id(named.clone()),
        )
        .build()?;
    broker.temporal().create_or_update(&a_evolution).send().await?.created()?;
    let b_evolution = TemporalEntity::builder()
        .id(b_id.clone())
        .entity_type(tqds_paged_type.clone())
        .property("speed", Property::builder(20).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&b_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let both = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_paged_type)
        .dataset_ids(vec![named.clone()])
        .send()
        .await?;
    expect_that!(both.items(), len(eq(2)));
    let a = with_id(both.items(), &a_id).or_fail()?;
    expect_that!(a, has_evolution("fuel", nth(0, any_attribute().value(40))));
    expect_that!(a, lacks_evolution("speed"));
    let b = with_id(both.items(), &b_id).or_fail()?;
    expect_that!(b.entity_type, eq(&OneOrMany::one(tqds_paged_type.clone())));
    expect_that!(b.attributes.len(), eq(0));
    let first = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_paged_type)
        .dataset_ids(vec![named.clone()])
        .count(ResultsCount::Include)
        .limit(1)
        .offset(0)
        .send()
        .await?;
    expect_that!(first.total(), some(eq(2)));
    expect_that!(first.items(), len(eq(1)));
    let first_id = first.items().first().map(|entity| entity.id.clone()).or_fail()?;
    let second = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_paged_type)
        .dataset_ids(vec![named])
        .count(ResultsCount::Include)
        .limit(1)
        .offset(1)
        .send()
        .await?;
    expect_that!(second.total(), some(eq(2)));
    expect_that!(second.items(), len(eq(1)));
    let second_id = second.items().first().map(|entity| entity.id.clone()).or_fail()?;
    expect_that!(second_id == first_id, eq(false));
    expect_that!([&a_id, &b_id].contains(&&second_id), eq(true));
    Ok(())
}
