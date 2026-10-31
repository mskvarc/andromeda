//! The scope is no partial update target.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, spelling_matcher::alone, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::PartialAttributeUpdate},
    model::{Attribute, AttributeKind, AttributeName, Change, Entity, ErrorType, InstanceDeletion, NgsiLdMediaType, Property, Scope, Value},
};

/// Pins: Partial Attribute Update naming scope, with the null or with a path as the value, answers 400
/// `BadRequestData` and the scope /a/b/c stands
/// Clause: 4.18, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9022_the_scope_is_no_partial_update_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_partial_scope_id = namespace.urn("Building", "partial-scope")?;
    let building_type = namespace.entity_type("Building")?;
    let building_partial_scope = Entity::builder()
        .id(building_partial_scope_id.clone())
        .entity_type(building_type)
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_partial_scope).send().await?.done()?;
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::Property));
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(
            &PartialAttributeUpdate::builder()
                .id(&building_partial_scope_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .attribute(&deletion)
                .build(),
        )?
        .uri(api_uri(&broker, &format!("entities/{building_partial_scope_id}/attrs/scope"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let patch = Attribute::from(Property::builder("/x").build()?).into_patch();
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request_2 = broker
        .unchecked(
            &PartialAttributeUpdate::builder()
                .id(&building_partial_scope_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .attribute(&Change::from(patch))
                .build(),
        )?
        .uri(api_uri(&broker, &format!("entities/{building_partial_scope_id}/attrs/scope"))?);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_partial_scope_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/a/b/c".parse::<Scope>()?)));
    Ok(())
}
