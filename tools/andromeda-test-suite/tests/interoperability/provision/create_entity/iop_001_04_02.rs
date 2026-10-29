//! ETSI `IOP_001_04_02`: five brokers; parking 2 is created whole in b1, in b3 without its `totalSpotsNumber`, which
//! b3 forwards to b5 alone, and in none of b2 and b4.

use crate::{
    node::Node,
    parking::{Sample, TOTAL_SPOTS},
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
        Sample::Parking2Full,
        &[
            (Node::B1, Holds::Whole),
            (Node::B2, Holds::Nothing),
            (Node::B3, Holds::Without(TOTAL_SPOTS)),
            (Node::B4, Holds::Nothing),
            (Node::B5, Holds::Only(TOTAL_SPOTS)),
        ],
    )
    .await
}

both_contexts!(
    run,
    iop_001_04_02_01_create_parking_2_with_default_context,
    iop_001_04_02_02_create_parking_2_with_user_context,
    pins = "Check that creating OffStreetParking:2 at b1 over five brokers puts its totalSpotsNumber alone in b5 and the rest in b1 and b3",
    clause = "4.3.3, 9, 10.2.2",
);
