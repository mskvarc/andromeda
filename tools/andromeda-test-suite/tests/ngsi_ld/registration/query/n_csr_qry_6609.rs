//! Lang is checked and leaves a language map member as registered.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    context_source_property::language_property_value,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::problem,
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::ErrorType,
    query::language_filter::LanguageFilter,
    registration::{CSourceRegistration, ContextSourcePropertyName, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type with lang fr answers 200 with the registration's
/// `LanguageProperty` member holding its whole `languageMap`, as the same query without lang does, and with a lang
/// of `en_US`, `fr,,en` or `fr;level=1` answers 400 `BadRequestData`
/// Clause: 6.8.3.2, 5.10.2.3, 5.10.2.5, 4.15
#[gtest]
#[tokio::test]
async fn n_csr_qry_6609_lang_is_checked_and_leaves_a_language_map_member_as_registered() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let fleet_label = language_property_value(&[("en", "Fleet"), ("fr", "Flotte")]);
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .property("fleetLabel", fleet_label.clone())
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let label_name = "fleetLabel".parse::<ContextSourcePropertyName>()?;
    let unfiltered = broker.registrations().query().entity_type(&vehicle_type).send().await?;
    let filtered = broker
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .lang("fr".parse::<LanguageFilter>()?)
        .send()
        .await?;
    for page in [&unfiltered, &filtered] {
        let registrations = page.items();
        expect_that!(registrations, len(eq(1)));
        expect_that!(registrations, nth(0, has_id(&registration_id)));
        expect_that!(
            registrations.first().and_then(|registration| registration.properties().get(&label_name)),
            some(eq(&fleet_label))
        );
    }
    let type_text = vehicle_type.to_string();
    // The client cannot write a lang that is no language filter, so the query string is written as given.
    for lang in ["en_US", "fr,,en", "fr;level=1"] {
        let request = exchange(
            &broker,
            Method::GET,
            &target("csourceRegistrations", &[("type", type_text.as_str()), ("lang", lang)]),
            &[("Accept", "application/json")],
            None,
        )?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "lang {lang}");
    }
    Ok(())
}
