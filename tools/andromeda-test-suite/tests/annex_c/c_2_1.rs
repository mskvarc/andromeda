//! Annex C.2.1: the property graph of figure C.2.1-1, a Vehicle parked at an `OffStreetParking` whose `isParked`
//! Relationship carries a `providedBy` sub-Relationship (clauses 4.5.1 and 4.5.3).

use crate::{
    served_context::{ServedContexts, linked, retrieved_ld},
    vehicle_example::{aggregated_context, create_vehicle_example},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use serde_json::json;

/// Pins: The Vehicle of figure C.2.1-1 and the Entities its Relationships target are created and read back with the
/// isParked Relationship, its providedBy sub-Relationship and the `OffStreetParking` it targets
/// Clause: C.2.1, 4.5.1, 4.5.3, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_1_0001_the_property_graph_round_trips() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = create_vehicle_example(&broker, &namespace).await?;
    let body = retrieved_ld(&broker, &vehicle.id).await?;
    let parking = namespace.urn("OffStreetParking", "Downtown1")?;
    expect_that!(body.pointer("/isParked/object"), some(eq(&json!(parking.as_str()))));
    expect_that!(
        body.pointer("/isParked/providedBy/object"),
        some(eq(&json!(namespace.urn("Person", "Bob")?.as_str())))
    );
    expect_that!(body.pointer("/isParked/observedAt"), some(eq(&json!("2017-07-29T12:00:04Z"))));
    let body = retrieved_ld(&broker, &parking).await?;
    expect_that!(body.pointer("/type"), some(eq(&json!(namespace.suffixed("OffStreetParking")))));
    served.stop();
    Ok(())
}
