//! ETSI `IOP_001_02_02`: b1 holds an inclusive registration of the spot counts at b2, a redirect registration of the
//! `location` at b3 and a redirect registration of the `location` of parking 2 at b4. Parking 2 is created in b1
//! without its `location`, in b2 with its spot counts alone, and in b3 and b4 with its `location` alone.

use crate::{
    node::Node,
    parking::{LOCATION, SPOT_COUNTS, Sample},
    payload::Payload,
    provision::create_entity::{Holds, create_at_b1},
    scenario::{ContextChoice, Scenario},
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    scenario.register(Node::B1, Payload::Inclusive2, Node::B2).await?;
    scenario.register(Node::B1, Payload::Redirect2, Node::B3).await?;
    scenario.register(Node::B1, Payload::Redirect3, Node::B4).await?;
    create_at_b1(
        &mut scenario,
        Sample::Parking2Full,
        &[
            (Node::B1, Holds::Without(LOCATION)),
            (Node::B2, Holds::Only(SPOT_COUNTS)),
            (Node::B3, Holds::Only(LOCATION)),
            (Node::B4, Holds::Only(LOCATION)),
        ],
    )
    .await
}

both_contexts!(
    run,
    iop_001_02_02_01_create_parking_2_with_default_context,
    iop_001_02_02_02_create_parking_2_with_user_context,
    pins = "Check that creating OffStreetParking:2 at b1 splits it between b1, the spot counts of b2 and the location of b3 and b4",
    clause = "4.3.3, 9, 10.2.2",
);
