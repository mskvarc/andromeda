//! Avg over two unit codes is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: an avg aggregation over Property instances recorded under two `unitCodes` is refused with `BadRequestData`
/// naming the attribute and its unit codes, and carries no aggregate
/// Clause: 4.5.19, 5.7.3.4, 5.2.5, 5.5.4
/// Recorded: clause 4.5.19 defines avg over the instance values and says nothing about instances whose `unitCode`
/// differs (clause 5.2.5 attaches the unit to each instance); Andromeda converts no units, so it refuses a
/// unit-sensitive aggregate spanning two units rather than label it with one of them
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0001_avg_over_two_unit_codes_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let scale_id = namespace.urn("Scale", 1)?;
    let scale_type = namespace.entity_type("Scale")?;
    let scale_evolution = TemporalEntity::builder()
        .id(scale_id.clone())
        .entity_type(scale_type)
        .property(
            "weight",
            Property::builder(5).unit_code(UnitCode::Kgm).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .property(
            "weight",
            Property::builder(7).unit_code(UnitCode::Grm).observed_at("2026-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&scale_evolution).send().await?.created()?;
    let refused = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&scale_id)
        .aggregation_methods("avg".parse()?)
        .send()
        .await;
    expect_that!(
        &refused,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("weight")
            .with_detail_containing("unit codes")
    );
    Ok(())
}
