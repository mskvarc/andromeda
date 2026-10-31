//! Pick with no narrowing input is too wide in get and post.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    written_temporal_query::{get_parameters, post},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{projection::Projection, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities whose only restriction besides the temporal query is a pick (or an
/// omit) is too wide and refused with `BadRequestData`, through the GET parameters and the POST body alike, and the
/// same pick beside a type selector is answered
/// Clause: 5.7.4.4, 6.18.3.2, 6.24.3.1, 4.21
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3030_pick_with_no_narrowing_input_is_too_wide_in_get_and_post() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_id = namespace.urn("Meter", 1)?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_evolution = TemporalEntity::builder()
        .id(meter_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&meter_evolution).send().await?.created()?;
    // The temporal query refuses a projection that nothing else narrows before sending, so these requests are sent as
    // written.
    let meter_id_text = meter_id.to_string();
    let time = [("timerel", "after"), ("timeAt", "2000-01-01T00:00:00Z")];
    for narrowing in [
        vec![("pick", "power")],
        vec![("omit", "power")],
        vec![("id", meter_id_text.as_str()), ("pick", "power")],
    ] {
        let parameters = [narrowing.as_slice(), time.as_slice()].concat();
        let answer = get_parameters(&broker, &parameters, "application/json").await?;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    for projection in [r#""pick": [ "power" ]"#, r#""omit": [ "power" ]"#] {
        let answer = post(
            &broker,
            "",
            &format!(r#"{{"type": "Query", {projection}, "temporalQ": {{"timerel": "after", "timeAt": "2000-01-01T00:00:00Z"}}}}"#),
            "application/json",
            "application/json",
        )
        .await?;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_projected(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&meter_type)
        .pick("id,power".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&meter_id)));
    Ok(())
}
