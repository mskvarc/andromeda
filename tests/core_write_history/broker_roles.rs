//! The two brokers of the stack, and a client for each.

use andromeda_test_client::{broker_endpoint::BrokerEndpoint, test_client::TestClient};
use std::env;

/// Environment variable naming the recording control broker's API base.
const RECORDING_BROKER_URL_ENV: &str = "ANDROMEDA_TEST_RECORDING_BROKER_URL";
/// The recording broker's API base when the environment names none.
const DEFAULT_RECORDING_BROKER_URL: &str = "http://127.0.0.1:1103/ngsi-ld/v1/";

/// The stack's other broker, named by what it does with a Core write. The
/// harness's own broker is the subject: it leaves Core writes out of history
/// and writes the Temporal API's in the write's own transaction.
#[derive(Clone, Copy, Debug)]
pub enum Broker {
    /// Records Core writes, the default.
    Recorded,
}

/// A client for `broker` bound to the tenant `harness` addresses, so writes
/// through either land in the same database and tenant.
pub fn client_for(harness: &TestClient, broker: Broker) -> TestClient {
    match broker {
        Broker::Recorded => sibling(harness, RECORDING_BROKER_URL_ENV, DEFAULT_RECORDING_BROKER_URL),
    }
}

/// A client for the broker `variable` names, addressing the tenant of `harness`.
fn sibling(harness: &TestClient, variable: &str, default: &str) -> TestClient {
    let url = env::var(variable).unwrap_or_else(|_| default.to_owned());
    let endpoint = BrokerEndpoint::parse(&url).expect("the broker URL is valid");
    TestClient::new(endpoint, harness.tenant().cloned())
}
