//! A quoted prefixed name orders as the term for the same IRI does.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    written_parameters::{send_written, written_request},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{QueryParameter, operation::QueryEntities},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, Lossless},
    query::{builders::SelectorTypeBuilder, ordering},
};

/// Pins: orderBy="ex:speed" under an @context holding the prefix ex orders the Cars as orderBy=speed does under an
/// @context holding the term speed for the same IRI, and orderBy=ex:speed is 400 `BadRequestData`
/// Clause: 4.6.2, 4.9, 4.23.3, 6.4.3.2
/// Recorded: clause 4.23.3 reads an `orderBy` path with the clause 4.9 `AttrName`; Andromeda spells a prefixed name
/// there as it does in q, as a `quotedStr`
#[gtest]
#[tokio::test]
async fn a_qry_ord_9014_a_quoted_prefixed_name_orders_as_the_term_for_the_same_iri_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = |members: String| -> Result<Context> { Ok(Context::Single(ContextEntry::Inline(members.parse::<InlineContext>()?))) };
    let prefixed = HostedContext::add(&broker, &context(format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#))?).await?;
    let termed = HostedContext::add(&broker, &context(format!(r#"{{"speed":"https://example.org/{namespace}/speed"}}"#))?).await?;
    let writer = broker.with_context(Context::Single(termed.entry()?))?;
    let car_type = namespace.entity_type("Car")?;
    let ids = [namespace.urn("Car", "slow")?, namespace.urn("Car", "middle")?, namespace.urn("Car", "fast")?];
    for (id, speed) in ids.iter().zip([20, 50, 80]) {
        let car = Entity::builder()
            .id(id.clone())
            .entity_type(car_type.clone())
            .property("speed", speed)
            .build()?;
        writer.entities().create(&car).send().await?.done()?;
    }
    let selection = SelectorTypeBuilder::from(&car_type).build()?;
    let by_speed = ordering::by("speed").build()?;
    let termed_query = QueryEntities::builder()
        .entity_type(&selection)
        .ordering(&by_speed)
        .format(Lossless::Normalized)
        .build();
    let termed_page = send_written(&writer, &termed_query, &[]).await?;
    expect_that!(termed_page.items(), ids_in_order(&ids));
    let query = QueryEntities::builder().entity_type(&selection).format(Lossless::Normalized).build();
    // The ordering model writes a name only as an unquoted AttrName, so an orderBy naming a prefixed name is set as
    // written.
    let prefixed_page = send_written(
        &broker.with_context(Context::Single(prefixed.entry()?))?,
        &query,
        &[(QueryParameter::OrderBy, r#""ex:speed""#)],
    )
    .await?;
    expect_that!(prefixed_page.items(), ids_in_order(&ids));
    let request = written_request(&broker, &query, &[(QueryParameter::OrderBy, "ex:speed")])?;
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
    );
    Ok(())
}
