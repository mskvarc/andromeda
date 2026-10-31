//! An expires at with nine fraction digits is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Register Context Source with an `expiresAt` of nine fractional-second digits answers 400 `BadRequestData`,
/// and no registration exists
/// Clause: 4.6.3, 5.2.9, 5.9.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_csr_cre_7606_an_expires_at_with_nine_fraction_digits_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid timestamp at expiresAt), so `expiresAt` is written as text.
    let expires_at: RawJson = r#""2099-03-04T05:06:07.123456789Z""#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("expiresAt", &expires_at);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
