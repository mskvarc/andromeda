//! Only the ngsi ld null urn deletes and its iri lookalike is data.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::{Iri, iri};
use ngsi_ld::{
    http::{headers::Accept, operation::MergeEntity},
    model::{Attribute, Entity, EntityFragment, EntityId, NgsiLdMediaType, OneOrMany},
};

/// The lookalike: the IRI the core context's `ngsi-ld` prefix makes of `ngsi-ld:null`.
const LOOKALIKE: Iri<&str> = iri!("https://uri.etsi.org/ngsi-ld/null");

/// Pins: Merge Entity deletes a member only for the URN urn:ngsi-ld:null; the IRI the core context's ngsi-ld prefix
/// makes of ngsi-ld:null is written as data, as a first-level member, as a Property value and as a Relationship
/// object
/// Clause: 3.1, 4.5.0, 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_0023_only_the_ngsi_ld_null_urn_deletes_and_its_iri_lookalike_is_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let person_id = namespace.urn("Person", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 80)
        .property("label", "fleet")
        .relationship("owner", person_id.clone())
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 80).build()?;
    let body = format!(
        r#"{{"speed": "{LOOKALIKE}", "label": {{"type": "Property", "value": "{LOOKALIKE}"}}, "owner": {{"type": "Relationship", "object": "{LOOKALIKE}"}}}}"#
    );
    // The model writes the lone concise value of `speed` with its type, which is not the spelling under test, so the body is sent as written.
    let request = broker
        .unchecked(&MergeEntity::builder().id(&vehicle_id).fragment(&fragment).build())?
        .body(Bytes::from(body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(LOOKALIKE.as_str())));
    expect_that!(entity, has_attribute("label", property().value(LOOKALIKE.as_str())));
    expect_that!(entity, has_attribute("owner", relationship()));
    let owner_object = entity
        .attributes
        .iter()
        .find(|(name, _)| name.as_str() == "owner")
        .and_then(|(_, instances)| match instances.first() {
            Attribute::Relationship(relationship) => Some(relationship.object()),
            Attribute::Property(_)
            | Attribute::GeoProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::VocabProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::ListRelationship(_) => None,
        });
    expect_that!(owner_object, some(not(eq(&OneOrMany::one("urn:ngsi-ld:null".parse::<EntityId>()?)))));
    expect_that!(owner_object, some(not(eq(&OneOrMany::one(person_id)))));
    let deletion = EntityFragment::builder().delete("speed").build()?;
    broker.entities().merge(&vehicle_id, &deletion).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, lacks_attribute("speed"));
    expect_that!(entity_2, has_attribute("label", property().value(LOOKALIKE.as_str())));
    Ok(())
}
