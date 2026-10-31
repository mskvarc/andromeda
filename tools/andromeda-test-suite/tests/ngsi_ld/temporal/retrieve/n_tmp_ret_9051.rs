//! A last n above the maximum whose observed instances fit is answered in full without the undated ones.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    header_matcher::absent,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentRange},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Retrieve Temporal Evolution of speed holding three observed instances and two with no `observedAt` answers
/// `lastN` 5000, above the broker's maximum, with 200, no Content-Range and the three observed instances only: the
/// instances with no `observedAt` neither appear nor cut the answer short
/// Clause: 5.7.3.4, 6.3.10, Table 6.19.3.1-2
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute with
/// `lastN` (clause 6.3.10 leaves the limit to the implementation)
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9051_a_last_n_above_the_maximum_whose_observed_instances_fit_is_answered_in_full_without_the_undated_ones() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", 1000)
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-01T02:00:00Z".parse()?))
        .property("speed", 2000)
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(5000)
        .send()
        .await?;
    expect_that!(headed.head.typed::<ContentRange>(), absent());
    let retrieved = &headed.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                nth(0, any_attribute().value(10)),
                nth(1, any_attribute().value(20)),
                nth(2, any_attribute().value(30)),
                not(contains(any_attribute().value(1000))),
                not(contains(any_attribute().value(2000)))
            )
        )
    );
    Ok(())
}
