//! A prefixed attribute name breaking the grammar after the colon is bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, member_count},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteAttribute},
    model::{AttributeName, Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: Delete Attribute addressing ex:bad-name under an @context defining the prefix ex answers 400 `BadRequestData`
/// and leaves the entity as it was
/// Clause: 4.6.2, 5.6.5.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name the request's @context reads as a compact IRI whose part after the colon breaks it
#[gtest]
#[tokio::test]
async fn o_att_del_9005_a_prefixed_attribute_name_breaking_the_grammar_after_the_colon_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_delete_prefixed_id = namespace.urn("Vehicle", "delete-prefixed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Single(ContextEntry::Inline(r#"{"ex": "http://example.org/"}"#.parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let vehicle_delete_prefixed = Entity::builder()
        .id(vehicle_delete_prefixed_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle_delete_prefixed).send().await?.done()?;
    // The model refuses the attribute name, so the operation is built for a placeholder and sent to the path as written.
    let request = linked
        .unchecked(
            &DeleteAttribute::builder()
                .id(&vehicle_delete_prefixed_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .build(),
        )?
        .uri(api_uri(&linked, &format!("entities/{vehicle_delete_prefixed_id}/attrs/ex:bad-name"))?);
    let answer = linked.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_delete_prefixed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(1)));
    expect_that!(member_count(entity), eq(3));
    Ok(())
}
