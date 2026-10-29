//! ETSI `IOP_001_04_01`: five brokers; the parking is created in b1 and b3 and in none of b2, b4 and b5.

use crate::{
    node::Node,
    parking::Sample,
    provision::create_entity::{Holds, create_at_b1},
    scenario::{ContextChoice, Scenario},
    topology::FIVE_BROKERS_WITH_TOTAL_SPOTS_AT_B5,
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    scenario.register_all(&FIVE_BROKERS_WITH_TOTAL_SPOTS_AT_B5).await?;
    create_at_b1(
        &mut scenario,
        Sample::Parking1Full,
        &[
            (Node::B1, Holds::Whole),
            (Node::B2, Holds::Nothing),
            (Node::B3, Holds::Whole),
            (Node::B4, Holds::Nothing),
            (Node::B5, Holds::Nothing),
        ],
    )
    .await
}

both_contexts!(
    run,
    iop_001_04_01_01_create_parking_1_with_default_context,
    iop_001_04_01_02_create_parking_1_with_user_context,
    pins = "Check that creating OffStreetParking:1 at b1 over five brokers creates it in b1 and b3 and in none of b2, b4 and b5",
    clause = "4.3.3, 9, 10.2.2",
);
