//! A sub attribute chain at the limit is recorded as observation metadata.

use andromeda_test_suite::{
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nesting::{sub_attribute_chain, sub_attribute_chain_matcher},
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Create Temporal Evolution of an observation of p carrying a chain of 32 sub-Attributes answers 201, and
/// Retrieve Temporal Evolution reads the chain back to its last sub-Attribute
/// Clause: 5.6.11.4, 5.7.3.4, 4.5.2
#[gtest]
#[tokio::test]
async fn a_lim_dep_9005_a_sub_attribute_chain_at_the_limit_is_recorded_as_observation_metadata() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let nesting_probe_evolution = TemporalEntity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type)
        .property(
            "p",
            Property::builder(1)
                .observed_at("2024-01-01T00:00:00Z".parse()?)
                .property("s", sub_attribute_chain(31)?),
        )
        .build()?;
    broker.temporal().create_or_update(&nesting_probe_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(retrieved.body(), has_evolution("p", nth(0, sub_attribute_chain_matcher(32))));
    Ok(())
}
