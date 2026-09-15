//! HTTP/3 is real TLS 1.3: a client that does not trust the broker's certificate is rejected.

use crate::client::{H3_ALPN, Http3Client, trusted_roots, wait_until_ready};
use rustls::RootCertStore;

#[tokio::test]
async fn a_client_not_trusting_the_certificate_is_rejected() {
    wait_until_ready().await;
    // An empty root store cannot verify the broker's self-signed certificate, so
    // the QUIC/TLS handshake must fail rather than the connection being accepted.
    let outcome = Http3Client::connect(RootCertStore::empty(), vec![H3_ALPN.to_vec()]).await;
    assert!(outcome.is_err(), "a client trusting no roots must not complete the HTTP/3 handshake");
}

#[tokio::test]
async fn a_client_offering_a_non_h3_alpn_is_rejected() {
    wait_until_ready().await;
    // The broker offers only the `h3` ALPN; a client that offers only `h2` over
    // QUIC shares no protocol, so the handshake must fail.
    let outcome = Http3Client::connect(trusted_roots(), vec![b"h2".to_vec()]).await;
    assert!(outcome.is_err(), "a client offering only a non-h3 ALPN must not complete the HTTP/3 handshake");
}
