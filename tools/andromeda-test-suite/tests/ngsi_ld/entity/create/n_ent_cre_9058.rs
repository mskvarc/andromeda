//! A malformed entity type name is refused naming the type grammar.

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

/// Pins: Create Entity whose `type` is the hyphenated name "Bio-geographicalRegion", alone or in an array beside a
/// valid name, answers 400 `BadRequestData` whose detail names `type` and says a type is a term, a compact IRI or an
/// absolute IRI, carries no decoder vocabulary, and stores nothing
/// Clause: 4.6.2, 5.2.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9058_a_malformed_entity_type_name_is_refused_naming_the_type_grammar() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let region_type = namespace.entity_type("Region")?;
    for (key, written) in [
        ("alone", r#""Bio-geographicalRegion""#),
        ("in-array", r#"["Building", "Bio-geographicalRegion"]"#),
    ] {
        let region_id = namespace.urn("Region", key)?;
        let region = Entity::builder().id(region_id.clone()).entity_type(region_type.clone()).build()?;
        // The model refuses it (clause 4.6.2 admits no hyphen in a name), so `type` is written as text.
        let malformed_type: RawJson = written.parse()?;
        let document = UncheckedDocument::from_entity(&region, Lossless::Normalized).set_member("type", &malformed_type);
        let request = broker.unchecked(&CreateEntity::builder().entity(&region).build())?.document(&document);
        let answer = broker.send_unchecked(request).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
                .with_detail_containing("'type'")
                .with_detail_containing("a name must be a term, a compact IRI or an absolute IRI")
                .with_detail_lacking("untagged enum")
                .with_detail_lacking("variant")
                .with_detail_lacking("EntityTypeValue"),
            "type {written}"
        );
        let retrieved = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&region_id)
            .send()
            .await;
        expect_that!(&retrieved, answered(StatusCode::NOT_FOUND), "type {written}");
    }
    Ok(())
}
