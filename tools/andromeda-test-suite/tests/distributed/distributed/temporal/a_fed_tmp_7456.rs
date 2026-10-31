//! A dataset a per-attribute holder left out is probed for the hop target on its own.

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
use ngsi_ld::model::{ErrorType, Property, evolution::TemporalEntity};
use wiremock::ResponseTemplate;

/// The path a source holds Parking `number` at, beneath its registration prefix.
fn parking_path(namespace: &Namespace, number: u32) -> Result<String> {
    Ok(format!("/a/ngsi-ld/v1/temporal/entities/{}", namespace.urn("Parking", number)?))
}

/// The source's answer for Parking `number`: `free` 0 observed at `observed` in the dataset `dataset`.
fn parking_answer(namespace: &Namespace, number: u32, observed: &str, dataset: &str) -> Result<ResponseTemplate> {
    let parking = TemporalEntity::builder()
        .id(namespace.urn("Parking", number)?)
        .entity_type(namespace.entity_type("Parking")?)
        .property(
            "free",
            Property::builder(0)
                .dataset_id(namespace.dataset_id("dataset", dataset)?)
                .observed_at(observed.parse()?),
        )
        .build()?;
    Ok(source_answer::temporal_entity(&parking))
}

/// The probe of Parking `number` the source answers: the latest instance before the window opens.
fn probe(namespace: &Namespace, number: u32) -> Result<SourceRequest> {
    Ok(SourceRequest::new(Method::GET, &parking_path(namespace, number)?)
        .query("timerel", "before")
        .query("timeAt", "2020-08-01T12:00:00Z")
        .query("timeproperty", "observedAt")
        .query("lastN", 1)
        .priority(1))
}

/// Pins: under `splitEntities` and `temporal_linked_retrieval` a between 12:00-13:00 query on broker E answers
/// `q=parkedIn{free}>0` for Vehicle 2 alone: broker B holds dataset d1 free 3 at 11:00 for both Parkings, a source
/// applying `lastN` per Attribute answers the probe across datasets with its d2 free 0 at 11:40 alone and, asked again
/// for d1 by `datasetId`, with d1 free 0 at 11:30 for Parking 1 and at 10:30 for Parking 2; the follow-up probes name
/// d1
/// Clause: 5.7.4.4, 4.9, 4.11, 4.5.5, 6.18.3.2
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension, on broker E alone, admits
/// the hop and judges the Linked Entity on its instances valid within the window, the one each stream held when it
/// opened counted as clause 4.11 counts it for the entities the query returns
#[gtest]
#[tokio::test]
async fn a_fed_tmp_7456_a_dataset_a_per_attribute_holder_left_out_is_probed_for_the_hop_target_on_its_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_e = broker::client(BrokerRole::E)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let source = ContextSource::start().await?;
    write_parking(&broker_b, &namespace, 1, 3, "2020-08-01T11:00:00Z", Some("d1")).await?;
    write_parking(&broker_b, &namespace, 2, 3, "2020-08-01T11:00:00Z", Some("d1")).await?;
    write_vehicles(&broker_b, &broker_e, &namespace).await?;
    let registration = register_peer(&broker_e, &namespace, broker::peer(PeerRole::B)?).await?;
    let _registration_cleanup = Cleanup::of(&broker_e).registration_of(&registration);
    let source_registration = register_parking_source(&broker_e, &namespace, source.endpoint("a")?).await?;
    let _source_cleanup = Cleanup::of(&broker_e).registration_of(&source_registration);
    let d1 = namespace.dataset_id("dataset", "d1")?;
    for (number, held_by_d1) in [(1_u32, "2020-08-01T11:30:00Z"), (2, "2020-08-01T10:30:00Z")] {
        source
            .answer_when(
                probe(&namespace, number)?.without_query("datasetId"),
                parking_answer(&namespace, number, "2020-08-01T11:40:00Z", "d2")?,
            )
            .await?;
        source
            .answer_when(
                probe(&namespace, number)?.query("datasetId", &d1),
                parking_answer(&namespace, number, held_by_d1, "d1")?,
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
    let follow_ups = source
        .received_matching(
            &SourceRequest::matching(Method::GET, &format!("/a/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Parking:{namespace}-.*"))
                .query("timeproperty", "observedAt")
                .query("datasetId", &d1),
        )
        .await?;
    expect_that!(follow_ups, len(eq(2)));
    Ok(())
}
