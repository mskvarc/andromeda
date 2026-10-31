//! A value key holding a space reads back from the temporal evolution.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// Pins: Retrieve Temporal Evolution of an Entity whose one instance has the value {"opening hours": "9-5"} answers
/// that instance with the key as written
/// Clause: 4.6.4, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9011_a_value_key_holding_a_space_reads_back_from_the_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shop_temporal_id = namespace.urn("Shop", "temporal")?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_temporal_evolution = TemporalEntity::builder()
        .id(shop_temporal_id.clone())
        .entity_type(shop_type)
        .property(
            "p",
            Property::builder(object([("opening hours", Value::from("9-5"))])).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&shop_temporal_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&shop_temporal_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("p", all!(len(eq(1)), nth(0, any_attribute().value_member(["opening hours"], "9-5"))))
    );
    Ok(())
}
