//! A merge naming an attribute the request's @context expands to no IRI is bad request data and changes nothing.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    example_prefix::{UNEXPANDABLE_NAME, under_example_prefix},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::MergeEntity},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, Value},
};

/// Pins: Merge Entity whose @context defines the prefix ex, setting temperature to 30 beside an attribute named ex:a{b,
/// answers 400 `BadRequestData` and the Entity keeps temperature 21
/// Clause: 4.6.2, 5.6.17.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_ent_mrg_9024_a_merge_naming_an_attribute_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "merge")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .property("temperature", Value::Number("21".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let writer = under_example_prefix(&broker)?;
    let fragment = EntityFragment::builder().property("temperature", 30).property("placeholder", 1).build()?;
    // The model refuses a name that expands to no IRI, so the attribute is renamed to it.
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.rename_member("placeholder", UNEXPANDABLE_NAME);
    let request = writer
        .unchecked(&MergeEntity::builder().id(&building_id).fragment(&fragment).build())?
        .document(&document);
    expect_that!(
        &writer.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("temperature", property().value(21)));
    Ok(())
}
