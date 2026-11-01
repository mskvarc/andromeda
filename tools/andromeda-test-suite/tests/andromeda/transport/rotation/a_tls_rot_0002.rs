//! A broken certificate file leaves the previous certificate in service.

use andromeda_test_suite::{
    config::BrokerRole,
    identity_rotation::{IdentityFiles, RELOAD_WINDOW, await_presented, presented_to_a_new_connection},
    tls_connection::Connection,
};
use googletest::prelude::*;
use tokio::time::sleep;

/// The serial number of the certificate the stack starts the broker with, mutual.crt.
const MUTUAL_SERIAL_NUMBER: &str = "7c:95:37:14:e8:e4:14:e8:b0:1c:f2:00:2f:ef:3c:ac:d0:0e:1a";

/// Pins: after the broker's certificate file is replaced by text that is no certificate, a new connection is still
/// shown the certificate in service before, once the broker has had time to reload
/// Clause: 6.2
/// Recorded: certificate reloading is Andromeda's; clause 6.2 names TLS only. The broker reloads its identity when
/// the directory holding its files changes, after `tls.reload_settle_after` (500 ms on this broker); a reload that fails keeps the previous identity in service
#[gtest]
#[tokio::test]
async fn a_tls_rot_0002_a_broken_certificate_file_leaves_the_previous_certificate_in_service() -> Result<()> {
    let connection = Connection::new().trusting("tls/ca.crt").presenting("tls/client.crt", "tls/client.key");
    let identity = IdentityFiles::of(BrokerRole::Mutual)?;
    await_presented(&connection, BrokerRole::Mutual, MUTUAL_SERIAL_NUMBER).await?;
    identity.replace_certificate(b"this is not a certificate\n")?;
    // A refused reload changes nothing a client sees, so the test waits out the time the reload takes.
    sleep(RELOAD_WINDOW).await;
    let presented = presented_to_a_new_connection(&connection, BrokerRole::Mutual).await?;
    expect_that!(presented.serial_number(), eq(MUTUAL_SERIAL_NUMBER));
    identity.install("tls/mutual.crt", "tls/mutual.key")?;
    Ok(())
}
