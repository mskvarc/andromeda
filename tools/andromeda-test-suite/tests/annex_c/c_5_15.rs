//! Annex C.5.15: a scope query naming two scopes answers the parkings in either (clauses 4.18 and 4.19).

use crate::{
    parking_example::{Parking, create_parkings, entity},
    printed_document::expect_flat,
    served_context::{ServedContexts, linked},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::Representation;

/// The three parkings of the example, in the scopes `/Madrid/Centro`, `/Madrid/Cortes` with `/Company894/UnitC`,
/// and `/Madrid/Salamanca`.
const SCOPED_PARKINGS: [Parking; 3] = [
    Parking {
        suffix: "Downtown1",
        scope: &["/Madrid/Centro"],
        name: "Downtown One",
        available: 121,
        reliability: 0.7,
        camera: "C1",
        total: 200,
        position: (-8.5, 41.2),
    },
    Parking {
        suffix: "Corte4",
        scope: &["/Madrid/Cortes", "/Company894/UnitC"],
        name: "Corte4",
        available: 121,
        reliability: 0.7,
        camera: "C1",
        total: 100,
        position: (-8.6, 41.3),
    },
    Parking {
        suffix: "Salamanca7",
        scope: &["/Madrid/Salamanca"],
        name: "Salamanca7",
        available: 121,
        reliability: 0.7,
        camera: "C1",
        total: 50,
        position: (-8.7, 41.4),
    },
];

/// Pins: Query Entities with scopeQ naming two scopes answers the two parkings in them, in any order, as C.5.15
/// prints
/// Clause: C.5.15, 4.18, 4.19, 5.7.2
#[gtest]
#[tokio::test]
async fn c_5_15_0001_a_scope_query_answers_the_parkings_in_either_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    create_parkings(&broker, &namespace, &SCOPED_PARKINGS).await?;
    // The scope query builder writes the disjunction with `|`; the annex writes it with `,`, so the target is
    // written as printed.
    let target = format!("entities?type={}&scopeQ=/Madrid/Centro,/Madrid/Cortes", namespace.suffixed("OffStreetParking"));
    let request = wire::request_to(&broker, Method::GET, &target)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    let body = serde_json::from_slice(answer?.body())?;
    expect_flat(
        body,
        SCOPED_PARKINGS[..2]
            .iter()
            .map(|parking| entity(&namespace, parking, Representation::Normalized))
            .collect(),
    )?;
    served.stop();
    Ok(())
}
