//! ETSI `IOP_003_01_01`: the query of the three-broker inclusive and exclusive topology, sent as a GET.

use crate::{consumption::query_entities::inclusive_and_exclusive, scenario::ContextChoice};
use googletest::prelude::*;
use ngsi_ld::http::operation::QueryMethod;

async fn run(context: ContextChoice) -> Result<()> {
    inclusive_and_exclusive::run(context, QueryMethod::Get).await
}

both_contexts!(
    run,
    iop_003_01_01_01_query_parkings_via_get_with_default_context,
    iop_003_01_01_02_query_parkings_via_get_with_user_context,
    pins = "Check that querying OffStreetParking from b1 via GET answers the spot counts of b2 and the location of b3",
    clause = "4.3.3, 9, 10.4.3",
);
