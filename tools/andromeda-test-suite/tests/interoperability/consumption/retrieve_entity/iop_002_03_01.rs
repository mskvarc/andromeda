//! ETSI `IOP_002_03_01`: b1 holds an auxiliary registration of the spot counts at b2 and inclusive registrations at
//! b3 and b4. Parking 1 lives in b1, b2 and b3; b2 holds a `location` but is auxiliary, and b3 holds none, so b1
//! answers its own entity.

use crate::{
    composition::expect_same,
    node::Node,
    parking::{Kind, Sample},
    scenario::{ContextChoice, Scenario},
    topology::AUXILIARY_AND_TWO_INCLUSIVES,
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let own = scenario.seed(Node::B1, Sample::Parking1WithoutLocation).await?;
    let auxiliary = scenario.seed(Node::B2, Sample::Parking1Full).await?;
    scenario.seed(Node::B3, Sample::Parking1WithoutLocation).await?;
    scenario.seed(Node::B4, Sample::Parking2Full).await?;
    scenario.register_all(&AUXILIARY_AND_TWO_INCLUSIVES).await?;
    let at_b1 = scenario.retrieve(Node::B1, &own.id, Kind::Parking).await?;
    expect_same(&scenario.retrieve_local(Node::B1, &own.id, Kind::Parking).await?, &own)?;
    expect_same(&scenario.retrieve_local(Node::B2, &own.id, Kind::Parking).await?, &auxiliary)?;
    expect_same(&scenario.retrieve_local(Node::B3, &own.id, Kind::Parking).await?, &own)?;
    expect_same(&at_b1, &own)
}

both_contexts!(
    run,
    iop_002_03_01_01_retrieve_parking_1_with_default_context,
    iop_002_03_01_02_retrieve_parking_1_with_user_context,
    pins = "Check that retrieving OffStreetParking:1 from b1 answers its own entity, without the location of the auxiliary b2",
    clause = "4.3.3, 9, 10.4.2",
);
