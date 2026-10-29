//! ETSI `IOP_002_03_02`: b1 holds an auxiliary registration of every parking at b2 and inclusive registrations at b3
//! and b4. Retrieving only `location` answers the `location` b3 holds.

use crate::{
    composition::{combining, expect_same, taking},
    node::Node,
    parking::{Kind, LOCATION, Sample},
    payload::Payload,
    scenario::{ContextChoice, Scenario},
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    scenario.seed(Node::B1, Sample::Parking1WithoutLocation).await?;
    scenario.seed(Node::B2, Sample::Parking1Full).await?;
    let located = scenario.seed(Node::B3, Sample::Parking1LocationAndName).await?;
    scenario.seed(Node::B4, Sample::Parking1WithoutLocation).await?;
    scenario.register(Node::B1, Payload::Auxiliary1, Node::B2).await?;
    scenario.register(Node::B1, Payload::Inclusive1, Node::B3).await?;
    scenario.register(Node::B1, Payload::Inclusive2, Node::B4).await?;
    let at_b1 = scenario.retrieve_attributes(Node::B1, &located.id, Kind::Parking, LOCATION).await?;
    let at_b3 = scenario.retrieve_local(Node::B3, &located.id, Kind::Parking).await?;
    expect_same(&at_b3, &located)?;
    expect_same(&at_b1, &combining(&located, &[taking(&at_b3, LOCATION)])?)
}

both_contexts!(
    run,
    iop_002_03_02_01_retrieve_parking_1_location_with_default_context,
    iop_002_03_02_02_retrieve_parking_1_location_with_user_context,
    pins = "Check that retrieving the location of OffStreetParking:1 from b1 answers the location b3 holds",
    clause = "4.3.3, 9, 10.4.2",
);
