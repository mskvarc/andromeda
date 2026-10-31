//! The null scope refuses only its own entity.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, partial_outcome::reported, report_matcher::batch_error, spelling_matcher::alone};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Scope, Value},
};

/// Pins: Batch Entity Create of one Entity with scope "urn:ngsi-ld:null" and one with scope /a/b/c answers 207 with
/// `BadRequestData` for the first alone, and the second is created with its scope
/// Clause: 4.18, 5.6.7.4
#[gtest]
#[tokio::test]
async fn n_bat_cre_9007_the_null_scope_refuses_only_its_own_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_null_id = namespace.urn("Building", "batch-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_path_id = namespace.urn("Building", "batch-path")?;
    let building_batch_null = Entity::builder()
        .id(building_batch_null_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    // The model refuses it (invalid scope), so `scope` is written as text.
    let scope: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document = UncheckedDocument::from_entity(&building_batch_null, Lossless::Normalized).set_member("scope", &scope);
    let building_batch_path = Entity::builder()
        .id(building_batch_path_id.clone())
        .entity_type(building_type.clone())
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    let document_2 = UncheckedDocument::from_entity(&building_batch_path, Lossless::Normalized);
    let entities = [building_batch_null, building_batch_path];
    let request = broker
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_batch_path_id)));
    expect_that!(
        &report.errors,
        nth(0, batch_error().entity(&building_batch_null_id).problem(ErrorType::BadRequestData))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_path_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/a/b/c".parse::<Scope>()?)));
    Ok(())
}
