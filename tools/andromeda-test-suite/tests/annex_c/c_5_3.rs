//! Annex C.5.3: a query by type with a `q` excluding the Mercedes answers the two other Vehicles (clauses 4.9 and
//! 5.7.2).

use crate::{
    brand_fleet::{create_brand_fleet, simplified_brand_vehicle},
    printed_document::expect_flat,
    served_context::{ServedContexts, linked, ok_body},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::{SelectorType, q::Query},
};

/// Pins: Query Entities with type, a q excluding Mercedes and format=simplified answers the two other Vehicles, in
/// any order, as C.5.3 prints them
/// Clause: C.5.3, 4.9, 4.5.4, 5.7.2
/// Corrected: selected-attribute: the Vehicles hold only brandName, the one Attribute the printed answer shows for a
/// request without attrs (5.7.2)
#[gtest]
#[tokio::test]
async fn c_5_3_0001_query_by_type_and_q_answers_the_printed_vehicles() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    create_brand_fleet(&broker, &namespace).await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .query_simplified()
            .entity_type(namespace.suffixed("Vehicle").parse::<SelectorType>()?)
            .q(r#"brandName!="Mercedes""#.parse::<Query>()?)
            .send()
            .await,
    )?;
    expect_flat(
        body,
        vec![
            simplified_brand_vehicle(&namespace, "B9211", "Volvo"),
            simplified_brand_vehicle(&namespace, "C7788", "Renault"),
        ],
    )?;
    served.stop();
    Ok(())
}
