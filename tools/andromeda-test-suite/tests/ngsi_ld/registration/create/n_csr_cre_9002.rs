//! A registration geometry off the axes is refused.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    geometry_matcher::geometry,
    namespace::Namespace,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{
        ErrorType,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{Position, Shape},
    },
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with a location at latitude 91 answers 400 `BadRequestData` naming the
/// refused value and is not stored, and Update Context Source Registration with an `operationSpace` at longitude
/// -181 answers 400 `BadRequestData` and leaves the registration without one
/// Clause: 5.9.2.4, 5.9.3.4, 4.7
#[gtest]
#[tokio::test]
async fn n_csr_cre_9002_a_registration_geometry_off_the_axes_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let refused_id = namespace.registration_id("refused")?;
    let kept_id = namespace.registration_id("kept")?;
    let probe_type = namespace.entity_type("RangeProbe")?;
    let information = || -> Result<RegistrationInfo> {
        Ok(RegistrationInfo::builder()
            .entities([EntityInfo::new(probe_type.clone()).with_id_pattern(namespace.id_pattern_ending(".+", ".*")?)])
            .build()?)
    };
    let refused = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(refused_id.clone())
        .information([information()?])
        .build()?;
    // The position model refuses latitude 91, so `location` is written as text.
    let location: RawJson = r#"{"type": "Point", "coordinates": [0, 91]}"#.parse()?;
    let document = UncheckedDocument::try_from(refused.render())?.set_member("location", &location);
    let request = broker.unchecked(&RegisterContextSource { registration: &refused })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("91")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&refused_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let kept = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(kept_id.clone())
        .information([information()?])
        .location(Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.registrations().register(&kept).send().await?;
    // The same refusal on an update: the operation space is outside the WGS 84 ranges.
    let request_2 = json_request_to(
        &broker,
        Method::PATCH,
        &format!("csourceRegistrations/{kept_id}"),
        r#"{"operationSpace": {"type": "Point", "coordinates": [-181, 0]}}"#,
    )?;
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("-181")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&kept_id)
        .send()
        .await?;
    expect_that!(retrieved.operation_space(), none());
    expect_that!(
        retrieved.location(),
        some(geometry().of_type("Point").ordinate([1], Value::Number("46.05".parse()?)))
    );
    broker.registrations().delete(&kept_id).send().await?;
    Ok(())
}
