//! A geometry keeps its bbox and foreign members through a temporal write.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution with an observed `GeoProperty` Polygon carrying a bbox and foreign
/// members answers 201, and the bbox and foreign members read back in the Temporal Evolution and in the current
/// Entity
/// Clause: 5.6.11, 4.6.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9002_a_geometry_keeps_its_bbox_and_foreign_members_through_a_temporal_write() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let parcel_id = namespace.urn("Parcel", 1)?;
    let parcel_type = namespace.entity_type("Parcel")?;
    let parcel_evolution = TemporalEntity::builder().id(parcel_id.clone()).entity_type(parcel_type).build()?;
    // The model refuses it (a geometry with foreign members), so `area` is written as text.
    let area: RawJson = r#"[{"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]], "bbox": [0.0, 0.0, 4.0, 4.0], "plotName": "plot 7", "survey": {"year": 2024, "marks": [1, 2]}}, "observedAt": "2026-01-01T00:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(parcel_evolution.render(Lossless::Normalized))?.set_member("area", &area);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&parcel_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&parcel_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "area",
            all!(
                len(eq(1)),
                nth(
                    0,
                    any_attribute()
                        .value_member(["type"], "Polygon")
                        .length_at(["bbox"], 4)
                        .value_member(["bbox", "2"], 4)
                        .value_member(["bbox", "3"], 4)
                        .value_member(["plotName"], "plot 7")
                        .value_member(["survey", "year"], 2024)
                        .length_at(["survey", "marks"], 2)
                        .value_member(["survey", "marks", "1"], 2)
                        .length_at(["coordinates", "0"], 4)
                )
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&parcel_id)
        .send()
        .await?;
    let entity = retrieved_2.body();
    expect_that!(
        entity,
        has_attribute(
            "area",
            any_attribute()
                .value_member(["type"], "Polygon")
                .length_at(["bbox"], 4)
                .value_member(["bbox", "2"], 4)
                .value_member(["bbox", "3"], 4)
                .value_member(["plotName"], "plot 7")
                .value_member(["survey", "year"], 2024)
                .length_at(["survey", "marks"], 2)
                .value_member(["survey", "marks", "1"], 2)
                .length_at(["coordinates", "0"], 4)
        )
    );
    Ok(())
}
