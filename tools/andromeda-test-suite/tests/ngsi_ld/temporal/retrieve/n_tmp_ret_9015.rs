//! A sub second window selects one instance.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity with timerel between 10:00:00.050Z and 10:00:00.200Z answers only
/// the instance observed at 10:00:00.123Z
/// Clause: 4.6.3, 4.11, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9015_a_sub_second_window_selects_one_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let t_window_id = namespace.urn("T", "window")?;
    let t_type = namespace.entity_type("T")?;
    let t_window_evolution = TemporalEntity::builder()
        .id(t_window_id.clone())
        .entity_type(t_type)
        .property("p", Property::builder(1).observed_at("2026-03-01T10:00:00.123Z".parse()?))
        .property("p", Property::builder(2).observed_at("2026-03-01T10:00:01.456Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&t_window_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&t_window_id)
        .temporal_q(temporal_query::between(
            "2026-03-01T10:00:00.050Z".parse()?,
            "2026-03-01T10:00:00.200Z".parse()?,
        ))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "p",
            all!(len(eq(1)), nth(0, any_attribute().value(1).observed_at("2026-03-01T10:00:00.123Z".parse()?)))
        )
    );
    Ok(())
}
