//! The concise Vehicle of Annex C.2.2: alone, with its linked Entities inline or flattened, and filtered to one
//! language (clauses 4.5.2.3, 4.5.3.3, 4.5.23 and 4.15).

use crate::{
    printed_document::{expect_flat, expect_with_any_context, sort_list_by},
    printed_vehicle::{bare_cities, bare_named, bare_persons, concise_parking, concise_vehicle, flattened, set_member},
    served_context::{ServedContexts, linked, ok_body},
    vehicle_example::{aggregated_context, create_vehicle_example},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::InlineJoin,
    model::{Lossless, NgsiLdMediaType},
    query::language_filter,
};
use serde_json::json;

/// Pins: Retrieve Entity with format=concise answers the Vehicle in the concise representation as C.2.2 prints it
/// Clause: C.2.2, 4.5.2.3, 4.5.3.3, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_2_0005_the_concise_vehicle() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = create_vehicle_example(&broker, &namespace).await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve(&vehicle.id)
            .form(Lossless::Concise)
            .send()
            .await,
    )?;
    expect_with_any_context(body, &concise_vehicle(&namespace));
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with format=concise and join=inline answers the concise Vehicle with each Relationship
/// carrying the Entities it targets as C.2.2 prints it
/// Clause: C.2.2, 4.5.23, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_2_0006_the_concise_vehicle_with_inline_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = create_vehicle_example(&broker, &namespace).await?;
    let mut body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve(&vehicle.id)
            .form(Lossless::Concise)
            .join(InlineJoin::Inline)
            .send()
            .await,
    )?;
    // The entities of a multi-object Relationship come in no specified order (clause 4.5.23.2).
    sort_list_by(&mut body, "/passengers/entity", "id");
    let mut expected = concise_vehicle(&namespace);
    set_member(&mut expected, "/isParked", "entity", concise_parking(&namespace))?;
    set_member(&mut expected, "/passengers", "entity", bare_persons(&namespace))?;
    set_member(&mut expected, "/route", "entityList", bare_cities(&namespace))?;
    expect_with_any_context(body, &expected);
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with format=concise and join=flat answers the concise Vehicle and every Entity it targets
/// as one flat array as C.2.2 prints it
/// Clause: C.2.2, 4.5.23, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_2_0007_the_concise_vehicle_with_flattened_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = create_vehicle_example(&broker, &namespace).await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve_flattened(&vehicle.id)
            .form(Lossless::Concise)
            .send()
            .await,
    )?;
    expect_flat(
        body,
        flattened(&namespace, concise_vehicle(&namespace), concise_parking(&namespace), bare_named),
    )?;
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with format=concise and lang=fr answers the concise Vehicle with its `LanguageProperty`
/// reduced to the French string as C.2.2 prints it
/// Clause: C.2.2, 4.5.18, 4.15, 5.7.1
/// Expected: passengers keeps object where the example prints the `ListRelationship` objectList (clause 4.5.22)
#[gtest]
#[tokio::test]
async fn c_2_2_0008_the_concise_vehicle_with_a_language_filter() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = create_vehicle_example(&broker, &namespace).await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve(&vehicle.id)
            .form(Lossless::Concise)
            .lang(language_filter::language("fr").build()?)
            .send()
            .await,
    )?;
    let mut expected = concise_vehicle(&namespace);
    set_member(&mut expected, "", "street", json!({"value": "Grand Place", "lang": "fr"}))?;
    expect_with_any_context(body, &expected);
    served.stop();
    Ok(())
}
