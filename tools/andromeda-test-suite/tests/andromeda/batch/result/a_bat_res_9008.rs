//! One unusable unit in a multi chunk batch loses no neighbour.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, partial_outcome::reported, problem::answered, report_matcher::batch_error};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson},
};

/// The entities of the batch, more than one 512-write commit chunk.
const ENTITIES: usize = 600;

/// The entity whose unit the model refuses, in the first chunk.
const UNUSABLE: usize = 299;

/// Pins: Batch Entity Upsert of 600 entities, more than one 512-write commit chunk, one of them in the first chunk
/// with unitCode Celsius, answers 207 with 599 successes and that one error, and stores its neighbours in both chunks
/// Clause: Table 5.2.5-1, 5.6.8.4
#[gtest]
#[tokio::test]
async fn a_bat_res_9008_one_unusable_unit_in_a_multi_chunk_batch_loses_no_neighbour() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let batch_entity_error_type = namespace.entity_type("BatchEntityError")?;
    let entities = (0..ENTITIES)
        .map(|n| -> Result<_> {
            Ok(Entity::builder()
                .id(namespace.urn("BatchEntityError", n)?)
                .entity_type(batch_entity_error_type.clone())
                .property("speed", Property::builder(1).unit_code(UnitCode::Cel))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    // The model refuses the unit Celsius, so `speed` of the one entity is written as text.
    let unusable_speed: RawJson = r#"{"type": "Property", "value": 1, "unitCode": "Celsius"}"#.parse()?;
    let documents = entities
        .iter()
        .enumerate()
        .map(|(position, entity)| {
            let document = UncheckedDocument::from_entity(entity, Lossless::Normalized);
            if position == UNUSABLE {
                document.set_member("speed", &unusable_speed)
            } else {
                document
            }
        })
        .collect::<Vec<_>>();
    let request = broker.unchecked(&BatchUpsert::builder().entities(&entities).build())?.documents(&documents);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, len(eq(ENTITIES - 1)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(
            0,
            batch_error()
                .entity(&namespace.urn("BatchEntityError", UNUSABLE)?)
                .problem(ErrorType::BadRequestData)
        )
    );
    for neighbour in [0, 298, 300, 511, 512, 599] {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&namespace.urn("BatchEntityError", neighbour)?)
            .send()
            .await?;
        expect_that!(retrieved.body().id, eq(&namespace.urn("BatchEntityError", neighbour)?));
    }
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&namespace.urn("BatchEntityError", UNUSABLE)?)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
