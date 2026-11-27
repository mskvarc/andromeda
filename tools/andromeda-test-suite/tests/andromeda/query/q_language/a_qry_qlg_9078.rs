//! A prefixed attribute before a sub-attribute and a prefixed member of a structured value read alike unquoted or
//! quoted.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    problem::answered,
    value::object,
    written_parameters::send_written,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, QueryParameter, operation::QueryEntities},
    model::{Context, ContextEntry, Entity, InlineContext, Lossless, Property, Value},
    query::builders::SelectorTypeBuilder,
};
use std::slice;

/// Pins: with a Car whose ex:speed was observed at 2026-07-01T00:00:00Z and whose address holds the member ex:zip
/// "10115", and a Car whose ex:speed was observed at 2025-07-01T00:00:00Z and whose address holds ex:zip "20095",
/// Query Entities under an @context holding the prefix ex selects the first Car with
/// q=ex:speed.observedAt>2026-01-01T00:00:00Z, q="ex:speed".observedAt>2026-01-01T00:00:00Z,
/// `q=address[ex:zip]=="10115"` and `q=address["ex:zip"]=="10115"`
/// Clause: 4.9, 4.6.2, 5.7.2.4
/// Recorded: Andromeda reads an unquoted name through `-`, `:`, `/` and `#` wherever an unquoted colon has no meaning
/// of its own, which takes in the attribute before a dot and a member of a trailing group, and ends it at the dot
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9078_a_prefixed_name_before_a_dot_or_in_a_member_path_reads_alike_unquoted_or_quoted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let prefix = format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#);
    let prefixed = HostedContext::add(&broker, &Context::Single(ContextEntry::Inline(prefix.parse::<InlineContext>()?))).await?;
    let writer = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(prefix.parse::<InlineContext>()?),
            ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned())?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let car_type = namespace.entity_type("Car")?;
    let recent_id = namespace.urn("Car", "recent")?;
    let cars = [
        Entity::builder()
            .id(recent_id.clone())
            .entity_type(car_type.clone())
            .property("ex:speed", Property::builder(80).observed_at("2026-07-01T00:00:00Z".parse()?))
            .property("address", object([("ex:zip", Value::from("10115"))]))
            .build()?,
        Entity::builder()
            .id(namespace.urn("Car", "older")?)
            .entity_type(car_type.clone())
            .property("ex:speed", Property::builder(20).observed_at("2025-07-01T00:00:00Z".parse()?))
            .property("address", object([("ex:zip", Value::from("20095"))]))
            .build()?,
    ];
    let created = writer.with_responses().entities().batch_create(&cars).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let reader = broker.with_context(Context::Single(prefixed.entry()?))?;
    let selection = SelectorTypeBuilder::from(&car_type).build()?;
    let query = QueryEntities::builder().entity_type(&selection).format(Lossless::Normalized).build();
    // The q model writes a name only as an unquoted AttrName, so a q naming a prefixed name is set as written.
    for q in [
        "ex:speed.observedAt>2026-01-01T00:00:00Z",
        r#""ex:speed".observedAt>2026-01-01T00:00:00Z"#,
        r#"address[ex:zip]=="10115""#,
        r#"address["ex:zip"]=="10115""#,
    ] {
        let page = send_written(&reader, &query, &[(QueryParameter::Q, q)]).await?;
        expect_that!(page.items(), ids_in_order(slice::from_ref(&recent_id)), "{q}");
    }
    Ok(())
}
