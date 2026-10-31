//! A registration reads back and bad or unknown ids are refused.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::PeerRole, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Context Source Registration of a registration created on broker A answers 200 with its id and
/// type; an id that is not a URI answers 400 `BadRequestData` and an unknown id 404 `ResourceNotFound`
/// Clause: 5.10.1.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_ret_7600_a_registration_reads_back_and_bad_or_unknown_ids_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1-.*")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_2.id(), eq(&registration_id));
    let registration_3 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_3.id(), eq(&registration_id));
    // The model refuses the id this request names, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::any()), Method::GET, "csourceRegistrations/not-a-uri")?;
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_2_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
