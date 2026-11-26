//! A merge writing a prefixed `objectType` holding a hyphen after the colon is bad request data and changes nothing.

use andromeda_test_suite::{
    broker,
    entity_matcher::lacks_attribute,
    example_prefix::{HYPHENATED_TYPE, under_example_prefix},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::MergeEntity},
    model::{Attribute, Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson, Relationship, VocabTerm},
};

/// Pins: Merge Entity whose @context defines the prefix ex, adding a Relationship owner whose objectType is
/// ex:bad-type, answers 400 `BadRequestData` and the Building holds no owner
/// Clause: 4.5.3.2, 4.6.2, 5.6.17.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar; an
/// objectType names entity types, so Andromeda holds it to the grammar an entity type is held to
#[gtest]
#[tokio::test]
async fn o_ent_mrg_9032_a_merge_writing_a_hyphenated_object_type_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "merged")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .property("temperature", 21)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let writer = under_example_prefix(&broker)?;
    let owner = Attribute::from(
        Relationship::builder(namespace.urn("Person", "o")?)
            .object_type("ex:Placeholder".parse::<VocabTerm>()?)
            .build()?,
    );
    // The model refuses an objectType outside the grammar of a name, so the member is set to it.
    let broken_owner: RawJson = UncheckedDocument::try_from(owner.render(Lossless::Normalized))?
        .set_text("objectType", HYPHENATED_TYPE)
        .to_string()
        .parse()?;
    let fragment = EntityFragment::builder().build()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("owner", &broken_owner);
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
    expect_that!(retrieved.body(), lacks_attribute("owner"));
    Ok(())
}
