//! A full tag is bad request data when a reached source does not advertise language tags.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with q name`[en-GB]`=="Lift" on a broker at the language-ranges level, under an inclusive
/// registration of a broker at the clause 4.9 grammar, answers `BadRequestData` naming the registration, because
/// that broker refuses a full language tag
/// Clause: 4.9, 5.7.2.4, 6.4.3.2
/// Recorded: Andromeda extension, not v1.9.1: the `q_target_languages` extension probes every Context Source a read
/// reaches, refuses a target language one of them does not admit, and otherwise runs the read at the lowest level
/// among them, warning when that drops the language-ranges level to tag equality
#[gtest]
#[tokio::test]
async fn a_qry_qlg_7604_a_full_tag_is_bad_request_data_when_a_reached_source_does_not_advertise_language_tags() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_f = broker::client(BrokerRole::F)?;
    let place_id = namespace.urn("Place", 1)?;
    let place_type = namespace.entity_type("Place")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let place_3_id = namespace.urn("Place", 3)?;
    let registration_id = namespace.registration_id(1)?;
    let place = Entity::builder()
        .id(place_id)
        .entity_type(place_type.clone())
        .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
        .build()?;
    broker_f.entities().create(&place).send().await?.done()?;
    let place_3 = Entity::builder()
        .id(place_3_id)
        .entity_type(place_type.clone())
        .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
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
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker_f).registration(&registration_id);
    let answer = broker_f
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&place_type)
        .q(q::attr("name").member("en-GB").eq("Lift"))
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .with_body_containing(&format!("urn:ngsi-ld:ContextSourceRegistration:{namespace}-1"))
    );
    Ok(())
}
