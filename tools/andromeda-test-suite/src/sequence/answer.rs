//! An answer as the checks read it: status, HTTP version, headers, body, the URL asked, the time it took and the
//! certificate the server presented.

use crate::sequence::peer_certificate::PeerCertificate;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Version, header::CONTENT_TYPE};
use std::time::Duration;

/// One answer to one request.
#[derive(Debug, Clone)]
pub struct Answer {
    /// The status code.
    pub status: u16,
    /// The HTTP version the answer came in.
    pub version: Version,
    /// The header fields.
    pub headers: HeaderMap,
    /// The body, byte for byte.
    pub body: Bytes,
    /// The URL the request was sent to.
    pub url: String,
    /// The time from sending the request to reading the whole answer.
    pub duration: Duration,
    /// The certificate the server presented; none over plain HTTP.
    pub certificate: Option<PeerCertificate>,
}

impl Answer {
    /// An answer with `status` and a JSON `body`, for the requests the test's own stub server answers.
    #[must_use]
    pub fn json(status: u16, body: String, url: String) -> Answer {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Answer {
            status,
            version: Version::HTTP_11,
            headers,
            body: Bytes::from(body),
            url,
            duration: Duration::ZERO,
            certificate: None,
        }
    }
}
