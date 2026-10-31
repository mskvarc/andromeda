//! Modify temporal instance with a fragment id spelling the id in upper case names the target.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::ModifyAttributeInstance},
    model::{Attribute, AttributeName, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
};

/// Pins: a Modify Attribute instance whose Fragment holds an id spelling the target's URN with the scheme and
/// namespace identifier in upper case names the target and is applied
/// Clause: 5.4, 5.5.4, 5.6.14.4, 6.22.3.1
/// Recorded: clause 5.4 asks the Fragment id to be "equal to the id of the target (mutated) NGSI-LD element"
/// without defining equality; Andromeda reads it as IETF RFC 8141 section 3.1 URN-equivalence, under which the
/// scheme and the namespace identifier compare without regard to case
#[gtest]
#[tokio::test]
async fn o_cmn_val_2719_modify_temporal_instance_with_a_fragment_id_spelling_the_id_in_upper_case_names_the_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed: AttributeName = "speed".parse()?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance_id = instance_id_at(evolution, &"speed".parse()?, 0)?;
    let modified = Attribute::from(Property::builder(129).observed_at("2020-09-01T12:03:00Z".parse()?).build()?);
    // The model writes no id with the modified instance, so the spelling under test is set on the document.
    let instance: RawJson = r#"[{"type": "Property", "value": 129, "observedAt": "2020-09-01T12:03:00Z"}]"#.parse()?;
    let document = UncheckedDocument::empty()
        .set_text("id", &format!("URN:NGSI-LD:Vehicle:{namespace}-1"))
        .set_member("speed", &instance);
    let request = broker
        .unchecked(
            &ModifyAttributeInstance::builder()
                .id(&vehicle_id)
                .name(&speed)
                .instance_id(&instance_id)
                .instance(&modified)
                .build(),
        )?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", nth(0, any_attribute().value(129))));
    Ok(())
}
