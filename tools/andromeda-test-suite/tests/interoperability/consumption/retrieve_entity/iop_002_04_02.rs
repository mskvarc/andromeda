//! ETSI `IOP_002_04_02`: five brokers; b1 answers parking 1 with the spot counts b2 holds, the `name` b3 holds and the
//! `location` b5 holds. The exclusive registration of b3 gives b5 the `location` of parking 1, so b4's is not used.

use crate::{
    composition::{combining, expect_same, taking},
    node::Node,
    parking::{Kind, LOCATION, NAME, SPOT_COUNTS, Sample},
    payload::Payload,
    scenario::{ContextChoice, Scenario},
    topology::Link,
};
use googletest::prelude::*;

/// b1 holds an auxiliary registration of every parking at b2 and an inclusive one at b3; b2 holds redirect
/// registrations of the `location` at b4 and of the `location` of parking 2 at b5; b3 holds an exclusive registration
/// of the `location` of parking 1 at b5.
const TOPOLOGY: [Link; 5] = [
    (Node::B1, Payload::Auxiliary1, Node::B2),
    (Node::B2, Payload::Redirect2, Node::B4),
    (Node::B2, Payload::Redirect3, Node::B5),
    (Node::B1, Payload::Inclusive1, Node::B3),
    (Node::B3, Payload::Exclusive3, Node::B5),
];

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let own = scenario.seed(Node::B1, Sample::Parking1WithoutLocation).await?;
    scenario.seed(Node::B2, Sample::Parking1WithoutLocation).await?;
    scenario.seed(Node::B3, Sample::Parking1WithoutLocation).await?;
    scenario.seed(Node::B4, Sample::Parking1Full).await?;
    let located = scenario.seed(Node::B5, Sample::Parking1LocationAndName).await?;
    scenario.register_all(&TOPOLOGY).await?;
    let at_b1 = scenario.retrieve(Node::B1, &own.id, Kind::Parking).await?;
    let at_b2 = scenario.retrieve_local(Node::B2, &own.id, Kind::Parking).await?;
    let at_b3 = scenario.retrieve_local(Node::B3, &own.id, Kind::Parking).await?;
    let at_b5 = scenario.retrieve_local(Node::B5, &own.id, Kind::Parking).await?;
    expect_same(&at_b2, &own)?;
    expect_same(&at_b3, &own)?;
    expect_same(&at_b5, &located)?;
    expect_same(
        &at_b1,
        &combining(&own, &[taking(&at_b2, SPOT_COUNTS), taking(&at_b3, NAME), taking(&at_b5, LOCATION)])?,
    )
}

both_contexts!(
    run,
    iop_002_04_02_01_retrieve_parking_1_with_default_context,
    iop_002_04_02_02_retrieve_parking_1_with_user_context,
    pins = "Check that retrieving OffStreetParking:1 from b1 over five brokers answers the spot counts of b2, the name of b3 and the location of b5",
    clause = "4.3.3, 9, 10.4.2",
);
