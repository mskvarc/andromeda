//! The broker reloads its identity when the files change, without a restart:
//! new connections see the rotated certificate, open ones keep their session,
//! and a broken rewrite leaves the previous certificate in service.

use crate::stack::{Endpoint, certificate_der, material, material_bytes, presented_leaf, trusting_the_stack};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};
use std::{
    fs,
    time::{Duration, Instant},
};
use tokio::time::sleep;

/// How long a rotation may take to reach new connections: the broker debounces
/// file events for 500 ms, and a bind mount may deliver them late.
const ROTATION_DEADLINE: Duration = Duration::from_secs(30);

/// The two certificate pairs the rotating broker alternates between.
const PAIRS: [(&str, &str); 2] = [("broker.crt", "broker.key"), ("rotated.crt", "rotated.key")];

/// The leaf a brand-new connection to the rotating broker is shown.
async fn leaf_for_a_new_connection() -> Vec<u8> {
    let response = trusting_the_stack()
        .build()
        .expect("client builds")
        .get(Endpoint::Rotating.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the rotating broker answers over TLS");
    presented_leaf(&response).expect("a TLS response names its peer certificate")
}

/// The generated pair presenting `leaf`, and the other one.
fn current_and_other(leaf: &[u8]) -> ((&'static str, &'static str), (&'static str, &'static str)) {
    let [first, second] = PAIRS;
    if certificate_der(first.0) == leaf { (first, second) } else { (second, first) }
}

/// Write `contents` beside the rotating broker's `file` and rename it into
/// place, the way a certificate manager does.
fn replace_rotating(file: &str, contents: &[u8]) {
    let target = material("rotating").join(file);
    let staging = material("rotating").join(format!(".{file}.staging"));
    fs::write(&staging, contents).expect("the staging file is written");
    fs::rename(&staging, &target).expect("the staging file is renamed into place");
}

/// Install one generated pair as the rotating broker's identity.
fn install((certificate, key): (&str, &str)) {
    replace_rotating("tls.key", &material_bytes(key));
    replace_rotating("tls.crt", &material_bytes(certificate));
}

/// Wait until new connections are shown `expected`, failing at the deadline.
async fn await_new_connections_shown(expected: &[u8]) {
    let started = Instant::now();
    while leaf_for_a_new_connection().await != expected {
        assert!(
            started.elapsed() < ROTATION_DEADLINE,
            "the rotated certificate was not presented within {ROTATION_DEADLINE:?}"
        );
        sleep(Duration::from_millis(250)).await;
    }
}

/// The status and leaf of one request over `client`'s pooled connection.
async fn request_over(client: &Client) -> (StatusCode, Option<Vec<u8>>) {
    let response = client
        .get(Endpoint::Rotating.url("info/sourceIdentity"))
        .send()
        .await
        .expect("the open connection keeps serving");
    (response.status(), presented_leaf(&response))
}

#[tokio::test]
async fn a_rotated_certificate_is_presented_to_new_connections_while_an_open_connection_survives() {
    let open = trusting_the_stack().build().expect("client builds");
    let (status, before) = request_over(&open).await;
    assert_eq!(status, StatusCode::OK);
    let before = before.expect("a TLS response names its peer certificate");
    let (current, next) = current_and_other(&before);

    install(next);
    await_new_connections_shown(&certificate_der(next.0)).await;

    let (status, still) = request_over(&open).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        still.as_deref(),
        Some(before.as_slice()),
        "the connection opened before the rotation keeps its session"
    );

    // The rotating copy is committed material; leave it as it was found.
    install(current);
    await_new_connections_shown(&before).await;
}

#[tokio::test]
async fn a_broken_certificate_file_leaves_the_previous_certificate_in_service() {
    let before = leaf_for_a_new_connection().await;
    let (current, _) = current_and_other(&before);

    replace_rotating("tls.crt", b"this is not a certificate\n");
    // Past the debounce, with a margin for the rejected reload to have run.
    sleep(Duration::from_secs(3)).await;
    assert_eq!(leaf_for_a_new_connection().await, before, "a broken rewrite is rejected");

    install(current);
    await_new_connections_shown(&before).await;
}
