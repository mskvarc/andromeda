//! A prefixed Relationship before a hop's brace and a prefixed attribute after its type list read alike unquoted or
//! quoted, while at the head of the braces an unquoted colon ends the type list.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    problem::answered,
    written_parameters::send_written,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, QueryParameter, operation::QueryEntities},
    model::{Context, ContextEntry, Entity, EntityId, InlineContext, Lossless},
    query::{Join, builders::SelectorTypeBuilder},
};
use std::{num::NonZeroU32, slice};

/// Pins: with one weather station linked through ex:sensor to a Device holding humidity 40, and another linked through
/// sensor to a Device of the same type holding humidity 40 and ex:humidity 40, Query Entities with join and joinLevel
/// 1 under an @context holding the prefix ex selects the first weather station with q=ex:sensor{<Device>:humidity}==40
/// and q="ex:sensor"{<Device>:humidity}==40, the second with q=sensor{<Device>:ex:humidity}==40,
/// q=sensor{<Device>:"ex:humidity"}==40 and q=sensor{"ex:humidity"}==40, and neither with
/// q=sensor{ex:humidity}==40, ex being read as an entity type
/// Clause: 4.9, 4.6.2, 5.7.2.4
/// Recorded: `X:y` alone never tells a type list from an absolute IRI, so Andromeda reads an unquoted name at the head
/// of a hop's braces as exactly an `AttrName` or `EntityType` and the colon after it as the end of the type list; as
/// the Relationship before the brace and after the list's colon it reads an unquoted name through its colons
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9077_a_prefixed_name_outside_the_head_of_a_hop_reads_alike_unquoted_or_quoted() -> Result<()> {
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
    let station_type = namespace.entity_type("WeatherStation")?;
    let device_type = namespace.entity_type("Device")?;
    let first_device_id = namespace.urn("Device", 1)?;
    let second_device_id = namespace.urn("Device", 2)?;
    let first_station_id = namespace.urn("WeatherStation", 1)?;
    let second_station_id = namespace.urn("WeatherStation", 2)?;
    let entities_written = [
        Entity::builder()
            .id(first_device_id.clone())
            .entity_type(device_type.clone())
            .property("humidity", 40)
            .build()?,
        Entity::builder()
            .id(second_device_id.clone())
            .entity_type(device_type.clone())
            .property("humidity", 40)
            .property("ex:humidity", 40)
            .build()?,
        Entity::builder()
            .id(first_station_id.clone())
            .entity_type(station_type.clone())
            .relationship("ex:sensor", first_device_id)
            .build()?,
        Entity::builder()
            .id(second_station_id.clone())
            .entity_type(station_type.clone())
            .relationship("sensor", second_device_id)
            .build()?,
    ];
    let created = writer.with_responses().entities().batch_create(&entities_written).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let reader = broker.with_context(Context::Single(prefixed.entry()?))?;
    let selection = SelectorTypeBuilder::from(&station_type).build()?;
    let joined = QueryEntities::builder()
        .entity_type(&selection)
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .format(Lossless::Normalized)
        .build();
    let device = device_type.as_str();
    let first = slice::from_ref(&first_station_id);
    let second = slice::from_ref(&second_station_id);
    let neither: &[EntityId] = &[];
    // The q model writes a name only as an unquoted AttrName, so a q naming a prefixed name is set as written.
    for (q, selected) in [
        (format!("ex:sensor{{{device}:humidity}}==40"), first),
        (format!(r#""ex:sensor"{{{device}:humidity}}==40"#), first),
        (format!("sensor{{{device}:ex:humidity}}==40"), second),
        (format!(r#"sensor{{{device}:"ex:humidity"}}==40"#), second),
        (r#"sensor{"ex:humidity"}==40"#.to_owned(), second),
        ("sensor{ex:humidity}==40".to_owned(), neither),
    ] {
        let page = send_written(&reader, &joined, &[(QueryParameter::Q, q.as_str())]).await?;
        expect_that!(page.items(), ids_in_order(selected), "{q}");
    }
    Ok(())
}
