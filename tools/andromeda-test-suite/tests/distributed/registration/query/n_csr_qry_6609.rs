//! Lang reduces a language map member to a property.

use andromeda_test_suite::{
    body_json::{json_equals, json_exists},
    broker,
    config::SuiteConfig,
    context_source_property::language_property_value,
    namespace::Namespace,
    raw_answer::send_raw,
    raw_call::target,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::registration::{CSourceRegistration, EntityInfo, RegistrationInfo};

/// Pins: Query Context Source Registrations by type with lang fr answers 200 with a `LanguageProperty` member of
/// the registration as a Property holding the French string and lang fr
/// Clause: 5.10.2.3, 4.15
#[gtest]
#[tokio::test]
async fn n_csr_qry_6609_lang_reduces_a_language_map_member_to_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .property("fleetLabel", language_property_value(&[("en", "Fleet"), ("fr", "Flotte")]))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let own = format!("$[?(@.id=='{registration_id}')]");
    let type_text = vehicle_type.to_string();
    // The reduced member is a Property, which the model does not read for a registration member, so the answer is read as received.
    let query = request_to(
        &broker,
        Method::GET,
        &target("csourceRegistrations", &[("type", type_text.as_str()), ("lang", "fr")]),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(query).await?;
    expect_that!(response.body(), json_exists(&own));
    expect_that!(response.body(), json_equals(&format!("{own}.fleetLabel.type"), "Property"));
    expect_that!(response.body(), json_equals(&format!("{own}.fleetLabel.value"), "Flotte"));
    expect_that!(response.body(), json_equals(&format!("{own}.fleetLabel.lang"), "fr"));
    Ok(())
}
