//! An idle keep-alive connection is closed after the idle bound.

use andromeda_test_suite::{config::BrokerRole, held_connection::HeldConnection};
use googletest::prelude::*;
use http::StatusCode;
use std::time::Duration;
use tokio::time::Instant;

/// The `server.idle_timeout` the timed broker runs with.
const IDLE_BOUND: Duration = Duration::from_secs(6);

/// Pins: a keep-alive connection whose one request was answered, and which then sends nothing, is closed without a
/// further answer once the idle bound has passed since the request, and not at the shorter head bound
/// Clause: 6.2
/// Recorded: Andromeda configuration, not v1.9.1: `server.idle_timeout` bounds how long a connection stays open with no
/// request in flight; RFC 9112 section 9.3 leaves a persistent connection open until either side closes it
#[gtest]
#[tokio::test]
async fn a_tls_con_0002_an_idle_keep_alive_connection_is_closed_after_the_idle_bound() -> Result<()> {
    // Taken before the connection opens, since the broker counts from its opening.
    let sent = Instant::now();
    let mut connection = HeldConnection::open(BrokerRole::Timed).await?;
    let head = connection.get_head_lines("types");
    connection.send(&format!("{head}\r\n")).await?;
    let answer = connection.response().await?;
    expect_that!(answer.status(), eq(StatusCode::OK));

    let (received, closed) = connection.until_closed().await?;

    expect_that!(received, is_empty());
    expect_that!(closed.duration_since(sent), ge(IDLE_BOUND));
    Ok(())
}
