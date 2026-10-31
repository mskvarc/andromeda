//! Attrs of another dataset keep the entity.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{NgsiLdMediaType, OneOrMany, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with attrs=speed and the dataset of fuel answers the Entity with its
/// id and type and no Attribute, normalized and with `temporalValues`, and with attrs naming an Attribute the
/// Entity never held answers nothing
/// Clause: 5.7.4.3, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9045_attrs_of_another_dataset_keep_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tqds_attrs_id = namespace.urn("TqdsAttrs", 1)?;
    let tqds_attrs_type = namespace.entity_type("TqdsAttrs")?;
    let named = namespace.dataset_id("Dataset", "named")?;
    let tqds_attrs_evolution = TemporalEntity::builder()
        .id(tqds_attrs_id.clone())
        .entity_type(tqds_attrs_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property(
            "fuel",
            Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?).dataset_id(named.clone()),
        )
        .build()?;
    broker.temporal().create_or_update(&tqds_attrs_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let normalized = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_attrs_type)
        .attrs(vec!["speed".parse()?])
        .dataset_ids(vec![named.clone()])
        .send()
        .await?;
    expect_that!(normalized.items(), len(eq(1)));
    expect_that!(normalized.items(), nth(0, has_id(&tqds_attrs_id)));
    let kept = normalized.items().first().or_fail()?;
    expect_that!(kept.entity_type, eq(&OneOrMany::one(tqds_attrs_type.clone())));
    expect_that!(kept.attributes.len(), eq(0));
    let values = json
        .temporal()
        .query_values(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_attrs_type)
        .attrs(vec!["speed".parse()?])
        .dataset_ids(vec![named.clone()])
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(values.items(), len(eq(1)));
    expect_that!(values.items(), nth(0, has_id(&tqds_attrs_id)));
    let kept_values = values.items().first().or_fail()?;
    expect_that!(kept_values.entity_type, eq(&OneOrMany::one(tqds_attrs_type.clone())));
    expect_that!(kept_values.attributes.len(), eq(0));
    let never = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_attrs_type)
        .attrs(vec!["never".parse()?])
        .dataset_ids(vec![named])
        .send()
        .await?;
    expect_that!(never.items(), is_empty());
    Ok(())
}
