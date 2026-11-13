//! A registration geometry written as a geo property is refused.

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

/// Pins: Create Context Source Registration with a `location` written as a `GeoProperty` around a geometry answers 400
/// `BadRequestData` naming `location` and is not stored, and Update Context Source Registration with an
/// `observationSpace` written so answers 400 `BadRequestData` and leaves the registration without one
/// Clause: 5.2.9, 5.5.4, 5.9.2.4, 5.9.3.4
#[gtest]
#[tokio::test]
async fn n_csr_cre_9007_a_registration_geometry_written_as_a_geo_property_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let refused_id = namespace.registration_id("refused")?;
    let kept_id = namespace.registration_id("kept")?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    let information = || -> Result<RegistrationInfo> {
        Ok(RegistrationInfo::builder()
            .entities([EntityInfo::new(probe_type.clone()).with_id_pattern(namespace.id_pattern_ending(".+", ".*")?)])
            .build()?)
    };
    let refused = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(refused_id.clone())
        .information([information()?])
        .build()?;
    // The registration model holds `location` only as a geometry, so the GeoProperty is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [14.5, 46.05]}}"#.parse()?;
    let document = UncheckedDocument::try_from(refused.render())?.set_member("location", &location);
    let request = broker.unchecked(&RegisterContextSource { registration: &refused })?.document(&document);
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("location")
    );
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&refused_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    let kept = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(kept_id.clone())
        .information([information()?])
        .location(Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.registrations().register(&kept).send().await?;
    let request_2 = json_request_to(
        &broker,
        Method::PATCH,
        &format!("csourceRegistrations/{kept_id}"),
        r#"{"observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.35, 48.85]}}}"#,
    )?;
    expect_that!(
        &broker.send_unchecked(request_2).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&kept_id)
        .send()
        .await?;
    expect_that!(retrieved.observation_space(), none());
    expect_that!(
        retrieved.location(),
        some(geometry().of_type("Point").ordinate([1], Value::Number("46.05".parse()?)))
    );
    broker.registrations().delete(&kept_id).send().await?;
    Ok(())
}
