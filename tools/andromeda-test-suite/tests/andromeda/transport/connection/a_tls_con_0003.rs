//! A request head arriving in parts inside the head bound is served.

use andromeda_test_suite::{config::BrokerRole, held_connection::HeldConnection};
use googletest::prelude::*;
use http::StatusCode;
use std::time::Duration;
use tokio::time::sleep;

/// Half the `server.request_head_timeout` of two seconds the timed broker runs with.
const PAUSE: Duration = Duration::from_secs(1);

/// Pins: a request head sent in two parts a second apart, its whole head arriving inside the head bound, is answered
/// 200
/// Clause: 6.2
/// Recorded: Andromeda configuration, not v1.9.1: `server.request_head_timeout` counts from the first octet of a head
/// to its end, so a head completed inside it is served as any other
#[gtest]
#[tokio::test]
async fn a_tls_con_0003_a_request_head_arriving_in_parts_inside_the_head_bound_is_served() -> Result<()> {
    let mut connection = HeldConnection::open(BrokerRole::Timed).await?;
    let head = connection.get_head_lines("types");
    connection.send(&head).await?;
    sleep(PAUSE).await;
    connection.send("\r\n").await?;

    let answer = connection.response().await?;

    expect_that!(answer.status(), eq(StatusCode::OK));
    Ok(())
}
