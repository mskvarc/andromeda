//! ETSI `IOP_002_01_02`: b1 holds an inclusive registration of the spot counts at b2 and an exclusive registration of
//! the `location` of parking 2 at b3. Parking 2 lives in b2 and b3, so b1 answers the spot counts of b2 and the
//! `location` of b3.

use crate::{
    composition::{combining, expect_same, taking},
    node::Node,
    parking::{Kind, LOCATION, SPOT_COUNTS, Sample},
    scenario::{ContextChoice, Scenario},
    topology::INCLUSIVE_AND_EXCLUSIVE,
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let counted = scenario.seed(Node::B2, Sample::Parking2WithoutLocation).await?;
    let full = scenario.seed(Node::B3, Sample::Parking2Full).await?;
    scenario.register_all(&INCLUSIVE_AND_EXCLUSIVE).await?;
    let at_b1 = scenario.retrieve(Node::B1, &counted.id, Kind::Parking).await?;
    let at_b2 = scenario.retrieve(Node::B2, &counted.id, Kind::Parking).await?;
    let at_b3 = scenario.retrieve(Node::B3, &counted.id, Kind::Parking).await?;
    expect_same(&at_b2, &counted)?;
    expect_same(&at_b3, &full)?;
    expect_same(&at_b1, &combining(&counted, &[taking(&at_b2, SPOT_COUNTS), taking(&at_b3, LOCATION)])?)
}

both_contexts!(
    run,
    iop_002_01_02_01_retrieve_parking_2_with_default_context,
    iop_002_01_02_02_retrieve_parking_2_with_user_context,
    pins = "Check that retrieving OffStreetParking:2 from b1 answers the spot counts of b2 and the location of b3",
    clause = "4.3.3, 9, 10.4.2",
);
