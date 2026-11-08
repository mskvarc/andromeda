//! A half-sent request head is answered 408 and closed after the head bound.

use andromeda_test_suite::{config::BrokerRole, held_connection::HeldConnection, raw_http::parse};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::CONNECTION};
use std::time::Duration;
use tokio::time::Instant;

/// The `server.request_head_timeout` the timed broker runs with.
const HEAD_BOUND: Duration = Duration::from_secs(2);

/// The `server.idle_timeout` the timed broker runs with.
const IDLE_BOUND: Duration = Duration::from_secs(6);

/// Pins: a connection that sends a request line and a `Host` field but never the empty line ending the head is answered
/// `408 Request Timeout` with `Connection: close` and closed once the head bound has passed, well before the idle
/// bound
/// Clause: 6.2
/// Recorded: Andromeda configuration, not v1.9.1: `server.request_head_timeout` bounds the wait for a request head;
/// RFC 9110 section 15.5.9 lets a server answer 408 when it no longer waits for a request, and RFC 9112 section 9.6
/// has it announce the close
#[gtest]
#[tokio::test]
async fn a_tls_con_0001_a_half_sent_request_head_is_answered_408_and_closed_after_the_head_bound() -> Result<()> {
    // Taken before the connection opens, since the broker counts from its opening.
    let sent = Instant::now();
    let mut connection = HeldConnection::open(BrokerRole::Timed).await?;
    let head = connection.get_head_lines("types");
    connection.send(&head).await?;

    let (received, closed) = connection.until_closed().await?;

    let answer = parse(&received)?;
    expect_that!(answer.status(), eq(StatusCode::REQUEST_TIMEOUT));
    expect_that!(answer.headers().get(CONNECTION), some(eq(&HeaderValue::from_static("close"))));
    expect_that!(closed.duration_since(sent), ge(HEAD_BOUND));
    expect_that!(closed.duration_since(sent), lt(IDLE_BOUND));
    Ok(())
}
