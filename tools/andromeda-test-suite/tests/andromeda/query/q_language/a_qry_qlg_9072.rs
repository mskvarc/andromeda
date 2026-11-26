//! A colon outside quotes ends a hop's type list, so a prefixed type or attribute inside the braces goes in quotes.

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
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext, Lossless},
    query::{Join, builders::SelectorTypeBuilder},
};
use std::{num::NonZeroU32, slice};

/// Pins: with Cars parked in an ex:Building and an ex:Warehouse, each holding ex:category "commercial",
/// q=isParkedIn{"ex:Building":"ex:category"}=="commercial" selects the Car parked in the Building, and
/// q=isParkedIn{ex:category}=="commercial" selects nothing, ex being read as an entity type
/// Clause: 4.9, 4.6.2, 5.7.2.4
/// Recorded: no unquoted `AttrName` or `EntityType` holds a colon, so Andromeda reads an unquoted colon at the head of
/// a hop as the end of its type list and a prefixed name there as a `quotedStr`
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9072_a_colon_outside_quotes_ends_a_hop_type_list() -> Result<()> {
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
    let building_id = namespace.urn("Building", "commercial")?;
    let warehouse_id = namespace.urn("Warehouse", "commercial")?;
    let in_building_id = namespace.urn("Car", "in-building")?;
    let entities_written = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type("ex:Building".parse::<EntityTypeName>()?)
            .property("ex:category", "commercial")
            .build()?,
        Entity::builder()
            .id(warehouse_id.clone())
            .entity_type("ex:Warehouse".parse::<EntityTypeName>()?)
            .property("ex:category", "commercial")
            .build()?,
        Entity::builder()
            .id(in_building_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_id)
            .build()?,
        Entity::builder()
            .id(namespace.urn("Car", "in-warehouse")?)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", warehouse_id)
            .build()?,
    ];
    let created = writer.with_responses().entities().batch_create(&entities_written).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let reader = broker.with_context(Context::Single(prefixed.entry()?))?;
    let selection = SelectorTypeBuilder::from(&car_type).build()?;
    let joined = QueryEntities::builder()
        .entity_type(&selection)
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .format(Lossless::Normalized)
        .build();
    // The q model writes a name only as an unquoted AttrName, so a q naming a prefixed name is set as written.
    let quoted = send_written(
        &reader,
        &joined,
        &[(QueryParameter::Q, r#"isParkedIn{"ex:Building":"ex:category"}=="commercial""#)],
    )
    .await?;
    expect_that!(quoted.items(), ids_in_order(slice::from_ref(&in_building_id)));
    let typed = send_written(&reader, &joined, &[(QueryParameter::Q, r#"isParkedIn{ex:category}=="commercial""#)]).await?;
    expect_that!(typed.items(), is_empty());
    Ok(())
}
