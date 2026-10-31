//! A registration location over the budget is 413.

use andromeda_test_suite::{
    broker,
    config::{Budget, SuiteConfig},
    generated_payload::polygon,
    namespace::Namespace,
    problem::{problem, problem_status, request_entity_too_large},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration whose location Polygon is one position over the budget answers 413
/// `RequestEntityTooLarge` naming the budget and location, and creates no registration
/// Clause: 6.3.2, 5.9.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_lim_pay_9009_a_registration_location_over_the_budget_is_413() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let budget = config.budget(Budget::Position)?;
    let registration_id = namespace.registration_id("location")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Probe")?).with_id_pattern(namespace.id_pattern_ending(".+", ".*")?)])
            .build()?])
        .build()?;
    // The model has no place for a registration location, so `location` is written as text.
    let location: RawJson = polygon(budget + 1)?.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("location", &location);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let refused = broker.send_unchecked(request).await;
    expect_that!(
        &refused,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing(&budget.to_string())
            .with_detail_containing("location")
    );
    expect_that!(problem_status(&refused), some(eq(413)));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
