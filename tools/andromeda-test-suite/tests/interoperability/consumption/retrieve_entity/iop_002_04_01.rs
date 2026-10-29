//! ETSI `IOP_002_04_01`: five brokers; b1 answers parking 1 with the spot counts b4 holds, reached through b2, and the
//! `location` b5 holds.

use crate::{
    composition::{combining, expect_same, taking},
    node::Node,
    parking::{Kind, LOCATION, SPOT_COUNTS, Sample},
    scenario::{ContextChoice, Scenario},
    topology::FIVE_BROKERS_WITH_TOTAL_SPOTS_AT_B5,
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let counted = scenario.seed(Node::B4, Sample::Parking1WithoutLocation).await?;
    let full = scenario.seed(Node::B5, Sample::Parking1Full).await?;
    scenario.register_all(&FIVE_BROKERS_WITH_TOTAL_SPOTS_AT_B5).await?;
    let at_b1 = scenario.retrieve(Node::B1, &counted.id, Kind::Parking).await?;
    let at_b4 = scenario.retrieve(Node::B4, &counted.id, Kind::Parking).await?;
    let at_b5 = scenario.retrieve(Node::B5, &counted.id, Kind::Parking).await?;
    expect_same(&at_b4, &counted)?;
    expect_same(&at_b5, &full)?;
    expect_same(&at_b1, &combining(&counted, &[taking(&at_b4, SPOT_COUNTS), taking(&at_b5, LOCATION)])?)
}

both_contexts!(
    run,
    iop_002_04_01_01_retrieve_parking_1_with_default_context,
    iop_002_04_01_02_retrieve_parking_1_with_user_context,
    pins = "Check that retrieving OffStreetParking:1 from b1 over five brokers answers the spot counts of b4 and the location of b5",
    clause = "4.3.3, 9, 10.4.2",
);
