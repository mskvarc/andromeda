//! ETSI `IOP_003_03_02`: b1 holds an auxiliary registration of every vehicle at b2 and inclusive registrations of every
//! parking at b3 and b4. Querying `OffStreetParking` and `Vehicle` for `location` answers parking 1 as b3 holds it,
//! parking 2 as b4 holds it and vehicle 1, which only b2 holds, as b2 holds it. Vehicle 2 is not answered: b4's
//! registration covers parkings.
//!
//! The case compares vehicle 1 in b1 with the whole vehicle file; the query asks for `location` alone, so b1, as b2,
//! answers the `location` of vehicle 1 alone.

use crate::{
    composition::{combining, expect_among, expect_ids, keeping, taking},
    node::Node,
    parking::{Kind, LOCATION, Sample},
    payload::Payload,
    scenario::{ContextChoice, Scenario},
};
use googletest::prelude::*;
use ngsi_ld::http::operation::QueryMethod;
use std::slice;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let first_vehicle = scenario.seed(Node::B2, Sample::Vehicle1).await?;
    let first_at_b3 = scenario.seed(Node::B3, Sample::Parking1LocationAndName).await?;
    let first_at_b4 = scenario.seed(Node::B4, Sample::Parking1Full).await?;
    let second_at_b4 = scenario.seed(Node::B4, Sample::Parking2Full).await?;
    let second_vehicle = scenario.seed(Node::B4, Sample::Vehicle2).await?;
    scenario.register(Node::B1, Payload::Auxiliary3, Node::B2).await?;
    scenario.register(Node::B1, Payload::Inclusive1, Node::B3).await?;
    scenario.register(Node::B1, Payload::Inclusive1, Node::B4).await?;
    let kinds = [Kind::Parking, Kind::Vehicle];
    let at_b1 = scenario.query(Node::B1, &kinds, LOCATION, QueryMethod::Get).await?;
    let at_b2 = scenario.query(Node::B2, &kinds, LOCATION, QueryMethod::Get).await?;
    let at_b3 = scenario.query(Node::B3, &kinds, LOCATION, QueryMethod::Get).await?;
    let at_b4 = scenario.query(Node::B4, &kinds, LOCATION, QueryMethod::Get).await?;
    expect_ids(&at_b1, &[first_at_b3.id.clone(), second_at_b4.id.clone(), first_vehicle.id.clone()])?;
    expect_ids(&at_b2, slice::from_ref(&first_vehicle.id))?;
    expect_ids(&at_b3, slice::from_ref(&first_at_b3.id))?;
    expect_ids(&at_b4, &[first_at_b4.id.clone(), second_at_b4.id.clone(), second_vehicle.id.clone()])?;
    let first_vehicle_b2 = expect_among(&at_b2, &first_vehicle.id, &keeping(&first_vehicle, LOCATION)?)?;
    let first_b3 = expect_among(&at_b3, &first_at_b3.id, &keeping(&first_at_b3, LOCATION)?)?;
    expect_among(&at_b4, &first_at_b4.id, &keeping(&first_at_b4, LOCATION)?)?;
    let second_b4 = expect_among(&at_b4, &second_at_b4.id, &keeping(&second_at_b4, LOCATION)?)?;
    expect_among(&at_b4, &second_vehicle.id, &keeping(&second_vehicle, LOCATION)?)?;
    expect_among(&at_b1, &first_at_b3.id, first_b3)?;
    expect_among(&at_b1, &second_at_b4.id, second_b4)?;
    expect_among(&at_b1, &first_vehicle.id, &combining(&first_vehicle, &[taking(first_vehicle_b2, LOCATION)])?)?;
    Ok(())
}

both_contexts!(
    run,
    iop_003_03_02_01_query_parkings_and_vehicles_with_default_context,
    iop_003_03_02_02_query_parkings_and_vehicles_with_user_context,
    pins = "Check that querying OffStreetParking and Vehicle for location from b1 answers parking 1 from b3, parking 2 from b4 and vehicle 1 from the auxiliary b2",
    clause = "4.3.3, 9, 10.4.3",
);
