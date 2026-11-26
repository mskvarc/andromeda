//! An append naming an attribute the request's @context expands to no IRI is bad request data and appends nothing.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    example_prefix::{UNEXPANDABLE_NAME, under_example_prefix},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AppendAttributes},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: Append Attributes whose @context defines the prefix ex, adding humidity beside an attribute named ex:a{b,
/// answers 400 `BadRequestData` and the Building keeps temperature 21 and holds no humidity
/// Clause: 4.6.2, 5.6.3.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name its @context expands to no absolute IRI, which JSON-LD expansion would otherwise drop
#[gtest]
#[tokio::test]
async fn o_att_app_9016_an_append_naming_an_attribute_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "appended")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .property("temperature", 21)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let writer = under_example_prefix(&broker)?;
    let fragment = EntityFragment::builder().property("humidity", 40).property("placeholder", 1).build()?;
    // The model refuses a name that expands to no IRI, so the attribute is renamed to it.
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.rename_member("placeholder", UNEXPANDABLE_NAME);
    let request = writer
        .unchecked(&AppendAttributes::builder().id(&building_id).fragment(&fragment).build())?
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
    expect_that!(retrieved.body(), lacks_attribute("humidity"));
    Ok(())
}
