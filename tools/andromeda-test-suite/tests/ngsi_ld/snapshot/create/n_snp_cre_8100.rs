//! A snapshot is created under its own or a generated id once.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Location,
    model::ErrorType,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Create Snapshot with an id answers 201 with an empty body and a Location naming that id; two creates
/// without an id answer Locations naming two different generated identifiers; a second create with the first id
/// answers 409 `AlreadyExists`
/// Clause: 5.16.1.4, 6.36.3.1
#[gtest]
#[tokio::test]
async fn n_snp_cre_8100_a_snapshot_is_created_under_its_own_or_a_generated_id_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let named = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    let created = broker.with_responses().snapshots().create(&named).send().await?;
    expect_that!(
        created
            .head
            .typed::<Location>()
            .valid()
            .map(|location| location.as_iri_ref().as_str().to_owned()),
        some(eq(&format!("/ngsi-ld/v1/snapshots/{snapshot_id}")))
    );
    let anonymous = Snapshot::builder()
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    let first = broker.with_responses().snapshots().create(&anonymous).send().await?;
    let first_location = first.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(first_location, some(starts_with("/ngsi-ld/v1/snapshots/")));
    let second = broker.with_responses().snapshots().create(&anonymous).send().await?;
    let second_location = second
        .head
        .typed::<Location>()
        .valid()
        .map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(second_location, some(starts_with("/ngsi-ld/v1/snapshots/")));
    expect_that!(second_location, not(eq(&first_location)));
    let again = broker.with_responses().snapshots().create(&named).send().await;
    expect_that!(&again, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    broker.snapshots().delete(&first.outcome.id().or_fail()?).send().await?;
    broker.snapshots().delete(&second.outcome.id().or_fail()?).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
