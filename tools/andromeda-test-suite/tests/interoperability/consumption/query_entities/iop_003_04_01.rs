//! ETSI `IOP_003_04_01`: the query of the five-broker topology, sent as a GET.

use crate::{consumption::query_entities::five_brokers, scenario::ContextChoice};
use googletest::prelude::*;
use ngsi_ld::http::operation::QueryMethod;

async fn run(context: ContextChoice) -> Result<()> {
    five_brokers::run(context, QueryMethod::Get).await
}

both_contexts!(
    run,
    iop_003_04_01_01_query_parkings_via_get_with_default_context,
    iop_003_04_01_02_query_parkings_via_get_with_user_context,
    pins = "Check that querying OffStreetParking from b1 via GET over five brokers answers the location of b5 and the spot counts of b4",
    clause = "4.3.3, 9, 10.4.3",
);
