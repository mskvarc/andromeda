//! Merge entity with a fragment id spelling only the scheme in upper case names the target.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, EntityId, NgsiLdMediaType},
};

/// Pins: a Merge Entity whose Fragment id spells only the URN scheme of the target in upper case names the target
/// and is applied
/// Clause: 5.4, 5.5.4, 5.6.17.4, 6.5.3.2
/// Recorded: clause 5.4 asks the Fragment id to be "equal to the id of the target (mutated) NGSI-LD element"
/// without defining equality; Andromeda reads it as IETF RFC 8141 section 3.1 URN-equivalence, under which the
/// scheme and the namespace identifier compare without regard to case
#[gtest]
#[tokio::test]
async fn o_cmn_val_2706_merge_entity_with_a_fragment_id_spelling_only_the_scheme_in_upper_case_names_the_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .id(EntityId::verbatim(&format!("URN:ngsi-ld:Vehicle:{namespace}-1"))?)
        .property("speed", 99)
        .build()?;
    let answer = broker.with_responses().entities().merge(&vehicle_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(99)));
    Ok(())
}
