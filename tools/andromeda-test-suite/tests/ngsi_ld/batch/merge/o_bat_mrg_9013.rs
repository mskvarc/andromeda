//! A batch merge item naming an attribute its @context expands to no IRI is a per-entity bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    example_prefix::{UNEXPANDABLE_NAME, under_example_prefix},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchMerge},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: Batch Entity Merge under an @context defining the prefix ex, setting temperature to 30 on two Buildings
/// holding 21, the first beside an attribute named ex:a{b, answers 207 with `BadRequestData` for the first, which keeps
/// 21, while the second reads 30
/// Clause: 4.6.2, 5.6.20.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_bat_mrg_9013_a_merge_item_naming_an_attribute_that_expands_to_no_iri_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let refused_id = namespace.urn("Building", "refused")?;
    let merged_id = namespace.urn("Building", "merged")?;
    let building_type = namespace.entity_type("Building")?;
    for id in [&refused_id, &merged_id] {
        let building = Entity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .property("temperature", 21)
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    let writer = under_example_prefix(&broker)?;
    let fragments = [
        EntityFragment::for_entity(refused_id.clone(), building_type.clone())
            .property("temperature", 30)
            .property("placeholder", 1)
            .build()?,
        EntityFragment::for_entity(merged_id.clone(), building_type)
            .property("temperature", 30)
            .build()?,
    ];
    // The model refuses a name that expands to no IRI, so the first item's attribute is renamed to it.
    let broken = UncheckedDocument::try_from(fragments[0].render(Lossless::Normalized))?.rename_member("placeholder", UNEXPANDABLE_NAME);
    let request = writer
        .unchecked(&BatchMerge::builder().fragments(&fragments).build())?
        .documents(&[broken, UncheckedDocument::try_from(fragments[1].render(Lossless::Normalized))?]);
    let report: BatchOperationResult = reported(writer.send_unchecked(request).await)?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&refused_id).problem(ErrorType::BadRequestData)));
    expect_that!(&report.success, nth(0, eq(&merged_id)));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let refused = json.entities().retrieve(&refused_id).send().await?;
    expect_that!(refused.body(), has_attribute("temperature", property().value(21)));
    let merged = json.entities().retrieve(&merged_id).send().await?;
    expect_that!(merged.body(), has_attribute("temperature", property().value(30)));
    Ok(())
}
