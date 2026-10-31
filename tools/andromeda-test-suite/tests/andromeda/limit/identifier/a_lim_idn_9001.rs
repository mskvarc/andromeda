//! An entity id one octet past the bound is refused by name.

use andromeda_test_suite::{broker, exact_length::padded, namespace::Namespace, problem::problem, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::UpsertOutcome,
    model::{Entity, EntityId, ErrorType},
};

/// Pins: Create Entity whose id is 1,025 octets answers 400 `BadRequestData` naming id, its 1025 bytes and the
/// 1024-byte bound, and Batch Entity Upsert of it answers 207 reporting that id with a `BadRequestData` naming id
/// and the bound
/// Clause: 5.5.4, 5.6.8.4, Table 5.2.17-1
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier, the Entity id
/// among them, at 1,024 octets, an index key
#[gtest]
#[tokio::test]
async fn a_lim_idn_9001_an_entity_id_one_octet_past_the_bound_is_refused_by_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let long_id = padded(&namespace.urn_text("LongReference", "long-id:"), 1025)?.parse::<EntityId>()?;
    let long_reference_type = namespace.entity_type("LongReference")?;
    let long_reference = Entity::builder().id(long_id.clone()).entity_type(long_reference_type.clone()).build()?;
    let answer = broker.with_responses().entities().create(&long_reference).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'id'")
            .with_detail_containing("1025 bytes")
            .with_detail_containing("1024 bytes")
    );
    let entities = [Entity::builder().id(long_id.clone()).entity_type(long_reference_type).build()?];
    let received = broker.with_responses().entities().batch_upsert(&entities).send().await?;
    expect_that!(received.status, eq(StatusCode::MULTI_STATUS));
    let report = match received.outcome {
        UpsertOutcome::Partial(report) => Some(report),
        UpsertOutcome::Created(_) | UpsertOutcome::Updated => None,
    }
    .or_fail()?;
    expect_that!(report.errors, len(eq(1)));
    expect_that!(
        report.errors,
        contains(
            batch_error()
                .entity(&long_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'id'")
                .with_detail_containing("1024 bytes")
        )
    );
    Ok(())
}
