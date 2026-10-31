//! The normalized Vehicle of Annex C.2.2: as created, with its linked Entities inline or flattened, and filtered to
//! one language (clauses 4.5.1, 4.5.23 and 4.15).

use crate::{
    printed_document::{expect_flat, expect_with_any_context, sort_list_by},
    printed_vehicle::{flattened, normalized_cities, normalized_named, normalized_parking, normalized_persons, normalized_vehicle, set_member},
    served_context::{ServedContexts, linked, ok_body, retrieved_ld},
    vehicle_example::{aggregated_context, create_vehicle_example, vehicle},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{http::operation::InlineJoin, model::NgsiLdMediaType, query::language_filter};
use serde_json::json;

/// Pins: The Vehicle exactly as C.2.2 prints it, a `ListProperty` carrying a unitCode included, is created and
/// retrieved in the normalized representation exactly as printed
/// Clause: C.2.2, 4.5.1, 4.5.2.2, 4.5.21.2, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_2_0001_the_normalized_vehicle_round_trips_as_printed() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = vehicle(&namespace, Some(UnitCode::Mmt))?;
    broker.entities().create(&vehicle).send().await?;
    let body = retrieved_ld(&broker, &vehicle.id).await?;
    let mut expected = normalized_vehicle(&namespace);
    set_member(&mut expected, "/tyreTreadDepths", "unitCode", json!("MMT"))?;
    expect_with_any_context(body, &expected);
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with join=inline answers the normalized Vehicle with each Relationship carrying the
/// Entities it targets as C.2.2 prints it
/// Clause: C.2.2, 4.5.1, 4.5.23, 5.7.1
/// Expected: providedBy keeps the type Relationship the inline example drops (clause 4.5.3.2)
#[gtest]
#[tokio::test]
async fn c_2_2_0002_the_normalized_vehicle_with_inline_linked_entities() -> Result<()> {
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
            .join(InlineJoin::Inline)
            .send()
            .await,
    )?;
    // The entities of a multi-object Relationship come in no specified order (clause 4.5.23.2).
    sort_list_by(&mut body, "/passengers/entity", "id");
    let mut expected = normalized_vehicle(&namespace);
    set_member(&mut expected, "/isParked", "entity", normalized_parking(&namespace))?;
    set_member(&mut expected, "/passengers", "entity", normalized_persons(&namespace))?;
    set_member(&mut expected, "/route", "entityList", normalized_cities(&namespace))?;
    expect_with_any_context(body, &expected);
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with join=flat answers the Vehicle and every Entity it targets as one flat array as C.2.2
/// prints it
/// Clause: C.2.2, 4.5.1, 4.5.23, 5.7.1
/// Expected: the flattened `OffStreetParking` carries no Entity-level observedAt or providedBy (clause 5.2.4)
#[gtest]
#[tokio::test]
async fn c_2_2_0003_the_normalized_vehicle_with_flattened_linked_entities() -> Result<()> {
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
            .send()
            .await,
    )?;
    let expected = flattened(&namespace, normalized_vehicle(&namespace), normalized_parking(&namespace), normalized_named);
    expect_flat(body, expected)?;
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with lang=fr answers the normalized Vehicle with its `LanguageProperty` reduced to the
/// French string as C.2.2 prints it
/// Clause: C.2.2, 4.5.18, 4.15, 5.7.1
/// Expected: isParked keeps the objectType the language-filtered example drops (clause 4.15)
#[gtest]
#[tokio::test]
async fn c_2_2_0004_the_normalized_vehicle_with_a_language_filter() -> Result<()> {
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
            .lang(language_filter::language("fr").build()?)
            .send()
            .await,
    )?;
    let mut expected = normalized_vehicle(&namespace);
    set_member(&mut expected, "", "street", json!({"type": "Property", "value": "Grand Place", "lang": "fr"}))?;
    expect_with_any_context(body, &expected);
    served.stop();
    Ok(())
}
