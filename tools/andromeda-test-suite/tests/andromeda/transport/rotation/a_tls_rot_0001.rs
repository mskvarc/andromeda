//! A rotated certificate reaches new connections while an open connection keeps its session.

use andromeda_test_suite::{
    config::BrokerRole,
    identity_rotation::{IdentityFiles, await_presented},
    presented_certificate::PresentedCertificate,
    tls_connection::Connection,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;

/// A certificate and key of the stack for the name broker-rotating, and the certificate's serial number.
struct Identity {
    /// The certificate file beneath the stack's directory.
    certificate: &'static str,
    /// The key file beneath the stack's directory.
    key: &'static str,
    /// The certificate's serial number.
    serial_number: &'static str,
}

/// The identity the stack starts the broker with.
const ROTATING: Identity = Identity {
    certificate: "tls/rotating.crt",
    key: "tls/rotating.key",
    serial_number: "79:91:fa:b5:fd:b6:76:48:23:6e:54:7e:70:ed:15:46:9c:b0:3b",
};

/// The identity the test rotates to.
const ROTATED: Identity = Identity {
    certificate: "tls/rotated.crt",
    key: "tls/rotated.key",
    serial_number: "70:5b:ce:26:09:a5:d2:7b:31:da:6b:23:38:69:53:56:0f:da:1d",
};

/// Pins: after the broker's identity files are replaced by another certificate and key, a new connection is shown the
/// new certificate while a connection opened before keeps being served under the certificate it was shown
/// Clause: 6.2
/// Recorded: certificate reloading is Andromeda's; clause 6.2 names TLS only. The broker reloads its identity when
/// the directory holding its files changes, after `tls.reload_settle_after` (500 ms on this broker)
#[gtest]
#[tokio::test]
async fn a_tls_rot_0001_a_rotated_certificate_reaches_new_connections_while_an_open_connection_keeps_its_session() -> Result<()> {
    let connection = Connection::new().trusting("tls/ca.crt");
    let identity = IdentityFiles::of(BrokerRole::Rotating)?;
    let open = connection.client(BrokerRole::Rotating).await?;
    let before = open.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(before.status, eq(StatusCode::OK));
    let shown = PresentedCertificate::of(before.head.extensions())?;
    // A run stopped between the rotation and the restore leaves the rotated identity in service.
    let (current, next) = if shown.serial_number() == ROTATED.serial_number {
        (ROTATED, ROTATING)
    } else {
        (ROTATING, ROTATED)
    };
    expect_that!(shown.serial_number(), eq(current.serial_number));
    identity.install(next.certificate, next.key)?;
    await_presented(&connection, BrokerRole::Rotating, next.serial_number).await?;
    let after = open.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(after.status, eq(StatusCode::OK));
    expect_that!(PresentedCertificate::of(after.head.extensions())?.serial_number(), eq(current.serial_number));
    identity.install(current.certificate, current.key)?;
    await_presented(&connection, BrokerRole::Rotating, current.serial_number).await
}
