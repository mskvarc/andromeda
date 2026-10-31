//! A registered id pattern scoping another option is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Register Context Source with an `idPattern` scoping the m option to a group answers 400 `BadRequestData`
/// and creates nothing
/// Clause: 4.9, 5.9.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options; Andromeda admits m, s and x
/// only set for the rest of a pattern, and refuses a registration declaring another use as `BadRequestData`
#[gtest]
#[tokio::test]
async fn a_csr_cre_0003_a_registered_id_pattern_scoping_another_option_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let hive_type = namespace.entity_type("Hive")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(hive_type).with_id_pattern("(?m:^hive)".parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    let answer = broker.with_responses().registrations().register(&registration).send().await;
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
