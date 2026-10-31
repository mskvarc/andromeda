//! Language range filtering applies across brokers that all advertise it.

use andromeda_test_suite::{
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
    http::{headers::Accept, operation::QueryEntities},
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType},
    query::{builders::SelectorTypeBuilder, q::Query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with q name`[en]`=="Lift" and with q name`[de-*-DE]`=="Lift" on a broker at the
/// language-ranges level, under an inclusive registration of a broker at the language-ranges level too, applies the
/// range on both brokers, answers without NGSILD-Warning, and selects en and en-US across the two and only de-DE
/// and de-Latn-DE for the wildcard range
/// Clause: 4.9, 5.7.2.4, 6.4.3.2
/// Recorded: Andromeda extension, not v1.9.1: the `q_target_languages` extension probes every Context Source a read
/// reaches, refuses a target language one of them does not admit, and otherwise runs the read at the lowest level
/// among them, warning when that drops the language-ranges level to tag equality
#[gtest]
#[tokio::test]
async fn a_qry_qlg_7603_language_range_filtering_applies_across_brokers_that_all_advertise_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_e = broker::client(BrokerRole::E)?;
    let broker_f = broker::client(BrokerRole::F)?;
    let place_type = namespace.entity_type("Place")?;
    let place_1_id = namespace.urn("Place", 1)?;
    let place_3_id = namespace.urn("Place", 3)?;
    let place_4_id = namespace.urn("Place", 4)?;
    for (holder, suffix, tag) in [
        (&broker_e, 1, "en-GB"),
        (&broker_e, 2, "de-Deva"),
        (&broker_f, 3, "en-US"),
        (&broker_f, 4, "de-Latn-DE"),
    ] {
        let place = Entity::builder()
            .id(namespace.urn("Place", suffix)?)
            .entity_type(place_type.clone())
            .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([(tag, "Lift")])?))
            .build()?;
        holder.entities().create(&place).send().await?.done()?;
    }
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::F)?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(place_type.clone()).with_id_pattern(namespace.id_pattern_ending("Place", "[34]")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker_e.registrations().register(&registration).send().await?;
    let json = broker_e.with_accept(Accept::new(NgsiLdMediaType::Json));
    let selector = SelectorTypeBuilder::from(&place_type).build()?;
    let english = "name[en]==\"Lift\"".parse::<Query>()?;
    let request = json.unchecked(&QueryEntities::builder().entity_type(&selector).q(&english).format(Lossless::Normalized).build())?;
    let answer = json.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK).without_header(HeaderName::from_static("ngsild-warning")));
    let found = entities(answer.as_ref().or_fail()?)?;
    expect_that!(found, len(eq(2)));
    expect_that!(found, contains(has_id(&place_1_id)));
    expect_that!(found, contains(has_id(&place_3_id)));
    // The model refuses the language range `de-*-DE` in a q path, so this query is sent as written.
    let request_2 = request_to(
        &json,
        Method::GET,
        &format!("entities?type=Place_{namespace}&q=name%5Bde-%2A-DE%5D%3D%3D%22Lift%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = json.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::OK).without_header(HeaderName::from_static("ngsild-warning")));
    let found_2 = entities(answer_2.as_ref().or_fail()?)?;
    expect_that!(found_2, len(eq(1)));
    expect_that!(found_2, contains(has_id(&place_4_id)));
    Ok(())
}
