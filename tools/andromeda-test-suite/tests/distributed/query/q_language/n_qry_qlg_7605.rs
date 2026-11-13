//! A q string holding an escaped control character selects among local and peer entities.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// The control characters a test writes into a String: line feed, tab, backspace, form feed, carriage return and
/// U+0001, which a q writes as `\n`, `\t`, `\b`, `\f`, `\r` and `\u0001`.
const CONTROLS: [char; 6] = ['\n', '\t', '\u{08}', '\u{0C}', '\r', '\u{01}'];

/// Pins: Query Entities on broker A by type with q name=="a\nb", and with each of \t, \b, \f, \r and \u0001 in place
/// of \n, over a local Entity and one broker B serves through an inclusive registration for each character, answers
/// the local and the peer Entity whose name holds that character only
/// Clause: 4.9, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_7605_a_q_string_holding_an_escaped_control_character_selects_among_local_and_peer_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let place_type = namespace.entity_type("Place")?;
    let registration_id = namespace.registration_id(1)?;
    for (index, control) in CONTROLS.into_iter().enumerate() {
        let local = Entity::builder()
            .id(namespace.urn("Place", format!("local-{index}"))?)
            .entity_type(place_type.clone())
            .property("name", format!("a{control}b").as_str())
            .build()?;
        broker.entities().create(&local).send().await?.done()?;
        let peer = Entity::builder()
            .id(namespace.urn("Place", format!("peer-{index}"))?)
            .entity_type(place_type.clone())
            .property("name", format!("a{control}b").as_str())
            .build()?;
        broker_b.entities().create(&peer).send().await?.done()?;
    }
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(place_type.clone()).with_id_pattern(namespace.id_pattern_ending("Place", "peer-[0-9]")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    for (index, control) in CONTROLS.into_iter().enumerate() {
        let page = broker
            .with_accept(Accept::any())
            .entities()
            .query()
            .entity_type(&place_type)
            .q(q::attr("name").eq(format!("a{control}b").as_str()))
            .send()
            .await?;
        expect_that!(page.items(), len(eq(2)), "character {index}");
        expect_that!(
            page.items(),
            contains(has_id(&namespace.urn("Place", format!("local-{index}"))?)),
            "character {index}"
        );
        expect_that!(
            page.items(),
            contains(has_id(&namespace.urn("Place", format!("peer-{index}"))?)),
            "character {index}"
        );
    }
    Ok(())
}
