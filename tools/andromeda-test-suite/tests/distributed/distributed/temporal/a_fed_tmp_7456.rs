//! A hop target holder applying lastN per Attribute is asked for the window alone.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    context_source::ContextSource,
    linked_temporal_hop::{expect_hop_selects, register_parking_source, register_peer, write_parking, write_vehicles},
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: under `splitEntities` and `temporal_linked_retrieval` a between 12:00-13:00 query on broker E answers
/// `q=parkedIn{free}>0` for Vehicle 2 alone: broker B holds dataset d1 free 3 for Parking 1 at 11:00, before the
/// window, and for Parking 2 at 12:30, inside it, and a second source registered for the Parkings is asked for no
/// request naming the dataset d1 and none before the window
/// Clause: 5.7.4.4, 4.9, 4.11, 4.5.5, 6.18.3.2
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension, on broker E alone, admits
/// the hop and judges the Linked Entity on its instances inside the window, as clause 5.7.4.4 steps S2 and S3 judge
/// the entities the query returns
#[gtest]
#[tokio::test]
async fn a_fed_tmp_7456_a_hop_target_holder_applying_last_n_per_attribute_is_asked_for_the_window_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_e = broker::client(BrokerRole::E)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let source = ContextSource::start().await?;
    write_parking(&broker_b, &namespace, 1, 3, "2020-08-01T11:00:00Z", Some("d1")).await?;
    write_parking(&broker_b, &namespace, 2, 3, "2020-08-01T12:30:00Z", Some("d1")).await?;
    write_vehicles(&broker_b, &broker_e, &namespace).await?;
    let registration = register_peer(&broker_e, &namespace, broker::peer(PeerRole::B)?).await?;
    let _registration_cleanup = Cleanup::of(&broker_e).registration_of(&registration);
    let source_registration = register_parking_source(&broker_e, &namespace, source.endpoint("a")?).await?;
    let _source_cleanup = Cleanup::of(&broker_e).registration_of(&source_registration);
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/a/.*").priority(10),
            source_answer::problem(
                StatusCode::NOT_FOUND,
                &source_answer::problem_without_detail(ErrorType::ResourceNotFound, "Entity not found", None),
            ),
        )
        .await?;
    expect_hop_selects(&broker_e, &namespace, &namespace.urn("Vehicle", 2)?).await?;
    let parkings = format!("/a/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Parking:{namespace}-.*");
    let before_the_window = source
        .received_matching(&SourceRequest::matching(Method::GET, &parkings).query("timerel", "before"))
        .await?;
    expect_that!(before_the_window, len(eq(0)));
    let by_dataset = source
        .received_matching(&SourceRequest::matching(Method::GET, &parkings).query("datasetId", &namespace.dataset_id("dataset", "d1")?))
        .await?;
    expect_that!(by_dataset, len(eq(0)));
    Ok(())
}
