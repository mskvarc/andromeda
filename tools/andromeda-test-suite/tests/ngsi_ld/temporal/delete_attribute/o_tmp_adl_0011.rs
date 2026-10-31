//! A prefixed attribute name breaking the grammar after the colon is bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    evolution_matcher::has_evolution,
    hosted_context::HostedContext,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteTemporalAttribute},
    model::{AttributeName, Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute from Temporal Evolution addressing ex:bad-name under an @context defining the prefix ex
/// answers 400 `BadRequestData` and leaves the recorded speed in place
/// Clause: 4.6.2, 5.6.13.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name the request's @context reads as a compact IRI whose part after the colon breaks it
#[gtest]
#[tokio::test]
async fn o_tmp_adl_0011_a_prefixed_attribute_name_breaking_the_grammar_after_the_colon_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Single(ContextEntry::Inline(r#"{"ex": "http://example.org/"}"#.parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The model refuses the attribute name, so the operation is built for a placeholder and sent to the path as written.
    let request = linked
        .unchecked(
            &DeleteTemporalAttribute::builder()
                .id(&vehicle_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .build(),
        )?
        .uri(api_uri(&linked, &format!("temporal/entities/{vehicle_id}/attrs/ex:bad-name"))?);
    let answer = linked.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", nth(0, property().value(1))));
    Ok(())
}
