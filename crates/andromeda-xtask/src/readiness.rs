//! Broker health-readiness polling shared by every runner that spawns a host
//! broker: a single probe of `/ngsi-ld/v1/health`, and a bounded wait built on it.

use reqwest::blocking::Client;
use std::{thread::sleep, time::Duration};

/// Whether a broker answered its health probe within the attempt budget.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Readiness {
    /// The broker answered a success status.
    Ready,
    /// The budget was exhausted without a success answer.
    Unready,
}

/// Probe one broker's health URL once, returning whether it answered a success
/// status. A transport error counts as not ready, never as a failure of the probe.
pub(crate) fn probe(client: &Client, url: &str) -> bool {
    client.get(url).send().is_ok_and(|response| response.status().is_success())
}

/// Poll one broker's health URL until it answers success or the attempt budget is
/// spent, sleeping one second between attempts.
pub(crate) fn await_health(client: &Client, url: &str, attempts: u16) -> Readiness {
    for _attempt in 0..attempts {
        if probe(client, url) {
            return Readiness::Ready;
        }
        sleep(Duration::from_secs(1));
    }
    Readiness::Unready
}
