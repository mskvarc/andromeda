//! Where the brokers under test answer.
//!
//! The base tier runs two brokers on one database: the broker under test, and
//! a second one (the trusted-proxy broker) that the cross-process cases use to
//! read what the first one stored.

use andromeda_test_client::broker_endpoint::BrokerEndpoint;
use std::env::var;

/// Names the second broker of the tier, which serves the same database.
const SECOND_BROKER_ENV: &str = "ANDROMEDA_TEST_TRUSTED_PROXY_BROKER_URL";

/// The second broker's API root when [`SECOND_BROKER_ENV`] names no other.
const DEFAULT_SECOND_BROKER: &str = "http://127.0.0.1:1057/ngsi-ld/v1/";

/// The absolute URL of `path` on the broker under test's NGSI-LD API.
pub fn broker_url(path: &str) -> String {
    BrokerEndpoint::from_environment()
        .expect("valid broker endpoint")
        .join(path)
        .expect("valid broker-relative test path")
        .to_string()
}

/// The absolute URL of `path` on the second broker's NGSI-LD API.
pub fn second_broker_url(path: &str) -> String {
    let root = var(SECOND_BROKER_ENV).unwrap_or_else(|_| DEFAULT_SECOND_BROKER.to_owned());
    format!("{}/{path}", root.trim_end_matches('/'))
}
