//! An expires at with nine fraction digits is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Update Context Source Registration with an `expiresAt` of nine fractional-second digits answers 400
/// `BadRequestData` and leaves the registration's `expiresAt` unchanged
/// Clause: 4.6.3, 5.9.3.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_csr_upd_7604_an_expires_at_with_nine_fraction_digits_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let expires_at = "2099-01-01T00:00:00Z".parse::<DateTime>()?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .expires_at(expires_at)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let path = format!("csourceRegistrations/{registration_id}");
    // The instant breaks the `DateTime` form the model reads, so the fragment is sent as written.
    let request = json_request_to(&broker, Method::PATCH, &path, r#"{"expiresAt": "2099-03-04T05:06:07.123456789Z"}"#)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.expires_at(), some(eq(&expires_at)));
    Ok(())
}
