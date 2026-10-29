//! ETSI `IOP_001_03_02`: a parking that carries a `name` and a `location` only is created in b1 and in b3, which is
//! registered for every parking, and not in b2 (auxiliary) or in b4, which is registered for the spot counts alone.

use crate::{
    node::Node,
    parking::Sample,
    provision::create_entity::{Holds, create_at_b1},
    scenario::{ContextChoice, Scenario},
    topology::AUXILIARY_AND_TWO_INCLUSIVES,
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    scenario.register_all(&AUXILIARY_AND_TWO_INCLUSIVES).await?;
    create_at_b1(
        &mut scenario,
        Sample::Parking1LocationAndName,
        &[
            (Node::B1, Holds::Whole),
            (Node::B2, Holds::Nothing),
            (Node::B3, Holds::Whole),
            (Node::B4, Holds::Nothing),
        ],
    )
    .await
}

both_contexts!(
    run,
    iop_001_03_02_01_create_partial_parking_1_with_default_context,
    iop_001_03_02_02_create_partial_parking_1_with_user_context,
    pins = "Check that creating a partial OffStreetParking:1 at b1 creates it in b1 and b3 and not in b2 and b4",
    clause = "4.3.3, 9, 10.2.2",
);
