//! The null scope beside a path is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    spelling_matcher::alone,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, MergeEntity},
    },
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson, Scope, Value},
};

/// Pins: Create Entity with scope `["/a/b/c","urn:ngsi-ld:null"]` answers 400 `BadRequestData` and creates nothing,
/// and Merge Entity with the same scope answers 400 `BadRequestData` and the scope /a/b/c stands
/// Clause: 4.18, 5.5.4, 5.6.1.4, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9028_the_null_scope_beside_a_path_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_mixed_create_id = namespace.urn("Building", "mixed-create")?;
    let building_type = namespace.entity_type("Building")?;
    let building_mixed_merge_id = namespace.urn("Building", "mixed-merge")?;
    let building_mixed_create = Entity::builder()
        .id(building_mixed_create_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (invalid scope), so `scope` is written as text.
    let scope: RawJson = r#"["/a/b/c", "urn:ngsi-ld:null"]"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_mixed_create, Lossless::Normalized).set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_mixed_create).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_mixed_create_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let building_mixed_merge = Entity::builder()
        .id(building_mixed_merge_id.clone())
        .entity_type(building_type.clone())
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_mixed_merge).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (invalid scope), so `scope` is written as text.
    let scope_2: RawJson = r#"["/a/b/c", "urn:ngsi-ld:null"]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("scope", &scope_2);
    let request_2 = broker
        .unchecked(&MergeEntity::builder().id(&building_mixed_merge_id).fragment(&fragment).build())?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_mixed_merge_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/a/b/c".parse::<Scope>()?)));
    Ok(())
}
