//! A time interval ending before it starts is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TimeInterval},
};

/// Pins: Create Context Source Registration with an `observationInterval` whose `endAt` is before its `startAt`
/// answers 400 `BadRequestData`
/// Clause: 5.9.2.4, 5.2.11
/// Recorded: Table 5.2.11-1 gives `startAt` and `endAt` as `DateTimes` and does not say the end may not precede the
/// start; Andromeda refuses such an interval
#[gtest]
#[tokio::test]
async fn o_csr_cre_6013_a_time_interval_ending_before_it_starts_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .observation_interval(TimeInterval::between("2030-01-01T00:00:00Z".parse()?, "2020-01-01T00:00:00Z".parse()?))
        .build()?;
    let answer = broker.with_responses().registrations().register(&registration).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
