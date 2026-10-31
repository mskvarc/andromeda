//! A source not advertising language range filtering makes the query fall back to tag equality with warning 199.

use andromeda_test_suite::{
    answer_warning::warns_first_with,
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    unchecked_page::entities,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::{Accept, WarningCode},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with q name`[en]`=="Lift" on a broker at the language-ranges level, under an inclusive
/// registration of a broker at the clause 4.9 grammar, evaluates the language by tag equality, so the local en-GB
/// entity is left out and the peer en entity is answered, and carries NGSILD-Warning code 199 naming the
/// registration
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 6.3.17
/// Recorded: Andromeda extension, not v1.9.1: the `q_target_languages` extension probes every Context Source a read
/// reaches, refuses a target language one of them does not admit, and otherwise runs the read at the lowest level
/// among them, warning when that drops the language-ranges level to tag equality
#[gtest]
#[tokio::test]
async fn a_qry_qlg_7601_a_source_not_advertising_language_range_filtering_makes_the_query_fall_back_to_tag_equality_with_warning_199() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_f = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let place_type = namespace.entity_type("Place")?;
    let place_3_id = namespace.urn("Place", 3)?;
    let registration_id = namespace.registration_id(1)?;
    let place = Entity::builder()
        .id(namespace.urn("Place", 1)?)
        .entity_type(place_type.clone())
        .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
        .build()?;
    broker_f.entities().create(&place).send().await?.done()?;
    let place_3 = Entity::builder()
        .id(place_3_id.clone())
        .entity_type(place_type.clone())
        .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Lift")])?))
        .build()?;
    broker_b.entities().create(&place_3).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(place_type.clone()).with_id_pattern(namespace.id_pattern_ending("Place", "[34]")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker_f.registrations().register(&registration).send().await?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker_f.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Place_{namespace}&q=name%5Ben%5D%3D%3D%22Lift%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_f.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        answered(StatusCode::OK).with_header_containing(
            HeaderName::from_static("ngsild-warning"),
            &format!("language range filtering not applied: registration {registration_id} does not advertise languageRangeFiltering")
        )
    );
    expect_that!(&answer, warns_first_with(WarningCode::MiscellaneousWarning));
    let found = entities(answer.as_ref().or_fail()?)?;
    expect_that!(found, len(eq(1)));
    expect_that!(found, contains(has_id(&place_3_id)));
    Ok(())
}
