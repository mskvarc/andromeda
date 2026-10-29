//! ETSI `IOP_001_03_01`: the auxiliary registration at b2 receives nothing; b3, registered for every parking, holds the
//! whole entity; b4, registered for the spot counts, holds those alone.

use crate::{
    node::Node,
    parking::{SPOT_COUNTS, Sample},
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
        Sample::Parking1Full,
        &[
            (Node::B1, Holds::Whole),
            (Node::B2, Holds::Nothing),
            (Node::B3, Holds::Whole),
            (Node::B4, Holds::Only(SPOT_COUNTS)),
        ],
    )
    .await
}

both_contexts!(
    run,
    iop_001_03_01_01_create_parking_1_with_default_context,
    iop_001_03_01_02_create_parking_1_with_user_context,
    pins = "Check that creating OffStreetParking:1 at b1 creates it whole in b1 and b3, with its spot counts alone in b4, and not in the auxiliary b2",
    clause = "4.3.3, 9, 10.2.2",
);
