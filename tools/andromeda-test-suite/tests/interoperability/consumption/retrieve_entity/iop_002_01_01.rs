//! ETSI `IOP_002_01_01`: b1 holds an inclusive registration of the spot counts at b2 and an exclusive registration of
//! the `location` of parking 2 at b3. Parking 1 lives in b2 alone, so b1 answers its spot counts as b2 holds them.

use crate::{
    composition::{combining, expect_same, taking},
    node::Node,
    parking::{Kind, SPOT_COUNTS, Sample},
    scenario::{ContextChoice, Scenario},
    topology::INCLUSIVE_AND_EXCLUSIVE,
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let first = scenario.seed(Node::B2, Sample::Parking1Full).await?;
    scenario.seed(Node::B2, Sample::Parking2Full).await?;
    scenario.seed(Node::B3, Sample::Parking2Full).await?;
    scenario.register_all(&INCLUSIVE_AND_EXCLUSIVE).await?;
    let at_b1 = scenario.retrieve(Node::B1, &first.id, Kind::Parking).await?;
    let at_b2 = scenario.retrieve(Node::B2, &first.id, Kind::Parking).await?;
    expect_same(&at_b2, &first)?;
    expect_same(&at_b1, &combining(&first, &[taking(&at_b2, SPOT_COUNTS)])?)
}

both_contexts!(
    run,
    iop_002_01_01_01_retrieve_parking_1_with_default_context,
    iop_002_01_01_02_retrieve_parking_1_with_user_context,
    pins = "Check that retrieving OffStreetParking:1 from b1 answers the spot counts of the entity b2 holds",
    clause = "4.3.3, 9, 10.4.2",
);
