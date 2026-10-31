//! Add temporal attributes with a fragment id spelling the id in upper case names the target.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AddTemporalAttributes},
    model::{Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: an Add Attributes to Temporal Evolution of an Entity whose Fragment id spells the target's URN with the
/// scheme and namespace identifier in upper case names the target and is applied
/// Clause: 5.4, 5.5.4, 5.6.12.4, 6.20.3.1
/// Recorded: clause 5.4 asks the Fragment id to be "equal to the id of the target (mutated) NGSI-LD element"
/// without defining equality; Andromeda reads it as IETF RFC 8141 section 3.1 URN-equivalence, under which the
/// scheme and the namespace identifier compare without regard to case
#[gtest]
#[tokio::test]
async fn o_cmn_val_0012_add_temporal_attributes_with_a_fragment_id_spelling_the_id_in_upper_case_names_the_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let added = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(99).observed_at("2020-09-02T12:00:00Z".parse()?))
        .build()?
        .attributes;
    // The model writes no id with the added instances, so the spelling under test is set on the document.
    let document = UncheckedDocument::try_from(added.render(Lossless::Normalized))?.set_text("id", &format!("URN:NGSI-LD:Vehicle:{namespace}-1"));
    let request = broker
        .unchecked(&AddTemporalAttributes::builder().id(&vehicle_id).attributes(&added).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                contains(any_attribute().value(99)),
                contains(any_attribute().value(120)),
                contains(any_attribute().value(80))
            )
        )
    );
    Ok(())
}
