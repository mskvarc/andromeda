//! ETSI `IOP_003_02_02`: b1 holds inclusive registrations of the spot counts of every parking and of every vehicle at
//! b2, a redirect registration of the `location` at b3 and redirect registrations of the `location` of parking 2 and of
//! vehicle 2 at b4. Querying `OffStreetParking` and `Vehicle` for `location` answers parking 1 as b3 holds it, parking 2
//! and vehicle 2 as b4 holds them and vehicle 1 as b2 holds it.
//!
//! The case compares the `location` b2 holds of parking 1 and the `location` b3 holds of parking 2 with the `location`
//! of the files b3 and b4 hold. The registrations give b1 no `location` from b2 for a parking, so the case checks
//! each source against the file it was given.

use crate::{
    composition::{combining, expect_among, expect_ids, keeping, taking},
    node::Node,
    parking::{Kind, LOCATION, Sample},
    payload::Payload,
    scenario::{ContextChoice, Scenario},
};
use googletest::prelude::*;
use ngsi_ld::http::operation::QueryMethod;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let first_vehicle = scenario.seed(Node::B2, Sample::Vehicle1).await?;
    let first_at_b2 = scenario.seed(Node::B2, Sample::Parking1Full).await?;
    let first_at_b3 = scenario.seed(Node::B3, Sample::Parking1LocationAndName).await?;
    let second_at_b3 = scenario.seed(Node::B3, Sample::Parking2Full).await?;
    let second_at_b4 = scenario.seed(Node::B4, Sample::Parking2LocationAndName).await?;
    let second_vehicle = scenario.seed(Node::B4, Sample::Vehicle2).await?;
    scenario.register(Node::B1, Payload::Inclusive2, Node::B2).await?;
    scenario.register(Node::B1, Payload::Inclusive3, Node::B2).await?;
    scenario.register(Node::B1, Payload::Redirect2, Node::B3).await?;
    scenario.register(Node::B1, Payload::Redirect3, Node::B4).await?;
    scenario.register(Node::B1, Payload::Redirect4, Node::B4).await?;
    let kinds = [Kind::Parking, Kind::Vehicle];
    let everything = [
        first_at_b2.id.clone(),
        second_at_b3.id.clone(),
        first_vehicle.id.clone(),
        second_vehicle.id.clone(),
    ];
    let at_b1 = scenario.query(Node::B1, &kinds, LOCATION, QueryMethod::Get).await?;
    let at_b2 = scenario.query(Node::B2, &kinds, LOCATION, QueryMethod::Get).await?;
    let at_b3 = scenario.query(Node::B3, &kinds, LOCATION, QueryMethod::Get).await?;
    let at_b4 = scenario.query(Node::B4, &kinds, LOCATION, QueryMethod::Get).await?;
    expect_ids(&at_b1, &everything)?;
    expect_ids(&at_b2, &[first_at_b2.id.clone(), first_vehicle.id.clone()])?;
    expect_ids(&at_b3, &[first_at_b3.id.clone(), second_at_b3.id.clone()])?;
    expect_ids(&at_b4, &[second_at_b4.id.clone(), second_vehicle.id.clone()])?;
    expect_among(&at_b2, &first_at_b2.id, &keeping(&first_at_b2, LOCATION)?)?;
    let first_vehicle_b2 = expect_among(&at_b2, &first_vehicle.id, &keeping(&first_vehicle, LOCATION)?)?;
    let first_b3 = expect_among(&at_b3, &first_at_b3.id, &keeping(&first_at_b3, LOCATION)?)?;
    expect_among(&at_b3, &second_at_b3.id, &keeping(&second_at_b3, LOCATION)?)?;
    let second_b4 = expect_among(&at_b4, &second_at_b4.id, &keeping(&second_at_b4, LOCATION)?)?;
    let second_vehicle_b4 = expect_among(&at_b4, &second_vehicle.id, &keeping(&second_vehicle, LOCATION)?)?;
    expect_among(&at_b1, &first_at_b2.id, first_b3)?;
    expect_among(&at_b1, &second_at_b3.id, second_b4)?;
    expect_among(&at_b1, &first_vehicle.id, &combining(&first_vehicle, &[taking(first_vehicle_b2, LOCATION)])?)?;
    expect_among(&at_b1, &second_vehicle.id, second_vehicle_b4)?;
    Ok(())
}

both_contexts!(
    run,
    iop_003_02_02_01_query_parkings_and_vehicles_with_default_context,
    iop_003_02_02_02_query_parkings_and_vehicles_with_user_context,
    pins = "Check that querying OffStreetParking and Vehicle for location from b1 answers each entity with the location of the broker its registration redirects to",
    clause = "4.3.3, 9, 10.4.3",
);
