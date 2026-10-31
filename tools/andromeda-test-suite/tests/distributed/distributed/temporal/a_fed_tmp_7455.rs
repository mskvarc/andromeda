//! A hop target deleted before the window never satisfies the hop.

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
use ngsi_ld::model::{
    AttributeKind,
    ErrorType,
    evolution::{DeletedInstance, TemporalEntity},
};

/// The path a source holds Parking `number` at, beneath its registration prefix.
fn parking_path(namespace: &Namespace, number: u32) -> Result<String> {
    Ok(format!("/a/ngsi-ld/v1/temporal/entities/{}", namespace.urn("Parking", number)?))
}

/// Pins: under `splitEntities` and `temporal_linked_retrieval` a between 12:00-13:00 query on broker E answers
/// `q=parkedIn{free}>0` for Vehicle 2 alone: broker B last wrote free 3 at 11:00 for both Parkings, and a source
/// answers the `deletedAt` probe of Parking 1 with free deleted at 11:30, after it, and of Parking 2 with free deleted
/// at 10:30, before it
/// Clause: 5.7.4.4, 4.9, 4.11, 4.5.7
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension, on broker E alone, admits
/// the hop and judges the Linked Entity on its instances valid within the window, the one each stream held when it
/// opened counted as clause 4.11 counts it for the entities the query returns
#[gtest]
#[tokio::test]
async fn a_fed_tmp_7455_a_hop_target_deleted_before_the_window_never_satisfies_the_hop() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_e = broker::client(BrokerRole::E)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let source = ContextSource::start().await?;
    write_parking(&broker_b, &namespace, 1, 3, "2020-08-01T11:00:00Z", None).await?;
    write_parking(&broker_b, &namespace, 2, 3, "2020-08-01T11:00:00Z", None).await?;
    write_vehicles(&broker_b, &broker_e, &namespace).await?;
    let registration = register_peer(&broker_e, &namespace, broker::peer(PeerRole::B)?).await?;
    let _registration_cleanup = Cleanup::of(&broker_e).registration_of(&registration);
    let source_registration = register_parking_source(&broker_e, &namespace, source.endpoint("a")?).await?;
    let _source_cleanup = Cleanup::of(&broker_e).registration_of(&source_registration);
    let parking_type = namespace.entity_type("Parking")?;
    for (number, deleted_at) in [(1_u32, "2020-08-01T11:30:00Z"), (2, "2020-08-01T10:30:00Z")] {
        let answer = TemporalEntity::builder()
            .id(namespace.urn("Parking", number)?)
            .entity_type(parking_type.clone())
            .deleted("free", DeletedInstance::builder(AttributeKind::Property, deleted_at.parse()?).build())
            .build()?;
        source
            .answer_when(
                SourceRequest::new(Method::GET, &parking_path(&namespace, number)?)
                    .query("timerel", "before")
                    .query("timeAt", "2020-08-01T12:00:00Z")
                    .query("timeproperty", "deletedAt")
                    .query("lastN", 1)
                    .without_query("datasetId")
                    .priority(1),
                source_answer::temporal_entity(&answer),
            )
            .await?;
    }
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
    let probes = source
        .received_matching(
            &SourceRequest::matching(Method::GET, &format!("/a/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Parking:{namespace}-.*"))
                .query("timeproperty", "deletedAt")
                .query("timeAt", "2020-08-01T12:00:00Z")
                // Clause 4.3.6.6: a registration without a jsonldContext entry gets the expanded name.
                .query("attrs", "https://uri.etsi.org/ngsi-ld/default-context/free"),
        )
        .await?;
    expect_that!(probes, len(eq(2)));
    Ok(())
}
