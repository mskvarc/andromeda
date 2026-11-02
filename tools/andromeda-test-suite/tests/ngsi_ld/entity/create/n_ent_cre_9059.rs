//! An entity type that is neither a string nor an array is refused as such.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose `type` is the number 42 answers 400 `BadRequestData` whose detail names `type` and says
/// a string or an array of strings was expected, differs from the refusal of a malformed name, carries no decoder
/// vocabulary, and stores nothing
/// Clause: 5.2.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9059_an_entity_type_that_is_neither_a_string_nor_an_array_is_refused_as_such() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let region_id = namespace.urn("Region", "numeric-type")?;
    let region = Entity::builder().id(region_id.clone()).entity_type(namespace.entity_type("Region")?).build()?;
    // The model refuses it (a type is a name), so `type` is written as text.
    let numeric_type: RawJson = "42".parse()?;
    let document = UncheckedDocument::from_entity(&region, Lossless::Normalized).set_member("type", &numeric_type);
    let request = broker.unchecked(&CreateEntity::builder().entity(&region).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'type'")
            .with_detail_containing("a string or an array of strings")
            .with_detail_lacking("a name must be a term")
            .with_detail_lacking("untagged enum")
            .with_detail_lacking("variant")
            .with_detail_lacking("EntityTypeValue")
    );
    let retrieved = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&region_id)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    Ok(())
}
