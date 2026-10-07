//! A payload over the size limit is refused with a problem detail.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.2 requires "Request Entity too large" (413) "when the HTTP input data stream provided by a client was too large i.e. too many bytes", and clause 5.5.3 makes every error body a problem detail.
//!
//! The refusal is answered before the upload is read. Closing the connection at once would make the TCP stack reset it over the unread upload, and a hyper-based client still writing would report the reset instead of the 413 it already received. The refusal therefore announces `Connection: close` (RFC 9110 clause 10.1.1) and the connection closes in stages (RFC 9112 clause 9.6).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::environment::TestEnvironment;
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    header::{CONNECTION, CONTENT_LENGTH},
};
use reqwest::{Method, StatusCode, Version};
use serde_json::Value;
use std::{
    io::{Error as IoError, ErrorKind},
    str,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use url::Url;

/// The broker's default `http.max_request_body`, which the test stacks keep.
const MAX_REQUEST_BODY: usize = 64 * 1024 * 1024;

/// How long one oversized exchange may take before the test gives up on it.
const EXCHANGE_DEADLINE: Duration = Duration::from_secs(60);

/// How many oversized uploads the hyper-based client sends in a row.
const HYPER_CLIENT_UPLOADS: usize = 20;

/// One body-taking operation and the media type its payload announces.
struct BodyTakingOperation {
    /// The request method.
    method: Method,
    /// The request target, relative to the NGSI-LD endpoint.
    path: &'static str,
    /// The payload's `Content-Type`.
    content_type: &'static str,
}

/// Every family of payload reader: an entity representation, a batch array, a
/// hosted `@context` document, and the plain JSON bodies of the entity map and
/// tenant updates.
const OPERATIONS: [BodyTakingOperation; 5] = [
    BodyTakingOperation {
        method: Method::POST,
        path: "entities",
        content_type: "application/json",
    },
    BodyTakingOperation {
        method: Method::POST,
        path: "entityOperations/upsert",
        content_type: "application/json",
    },
    BodyTakingOperation {
        method: Method::POST,
        path: "jsonldContexts",
        content_type: "application/json",
    },
    BodyTakingOperation {
        method: Method::PATCH,
        path: "entityMaps/urn:ngsi-ld:EntityMap:too-large",
        content_type: "application/json",
    },
    BodyTakingOperation {
        method: Method::PATCH,
        path: "tenants/too-large",
        content_type: "application/json",
    },
];

/// What a client that reads while it writes received for an oversized upload.
struct RawAnswer {
    /// The response status.
    status: StatusCode,
    /// The response headers.
    headers: HeaderMap,
    /// The response content, as long as its `Content-Length` says.
    body: Vec<u8>,
    /// `Ok` when the broker ended its side of the connection after the
    /// response; the kind of failure when reading broke off, typically on a
    /// reset connection.
    read_end: Result<(), ErrorKind>,
}

/// Upload one byte more than the limit over a bare HTTP/1.1 connection, reading
/// the response while the upload is still being written, as RFC 9112 clause 9.5
/// asks of a client.
async fn upload_reading_while_writing(url: &Url, method: &Method, content_type: &str) -> RawAnswer {
    let host = url.host_str().expect("the broker URL names a host");
    let port = url.port_or_known_default().expect("the broker URL has a port");
    let stream = TcpStream::connect((host, port)).await.expect("the broker accepts a connection");
    let (mut reader, mut writer) = stream.into_split();
    let head = format!(
        "{method} {} HTTP/1.1\r\nhost: {host}:{port}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\r\n",
        url.path(),
        MAX_REQUEST_BODY + 1
    );
    let upload = tokio::spawn(async move {
        writer.write_all(head.as_bytes()).await?;
        let chunk = vec![b' '; 64 * 1024];
        let mut remaining = MAX_REQUEST_BODY + 1;
        while remaining > 0 {
            let length = remaining.min(chunk.len());
            writer.write_all(&chunk[..length]).await?;
            remaining -= length;
        }
        // Keep the write side open: a half-closed request would end the
        // exchange for reasons of its own.
        Ok::<_, IoError>(writer)
    });

    let mut received = Vec::new();
    let mut buffer = [0_u8; 8192];
    let read_end = loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break Ok(()),
            Ok(read) => received.extend_from_slice(&buffer[..read]),
            Err(error) => break Err(error.kind()),
        }
    };
    // The upload ends when the broker closes the connection under it, which is
    // expected; only the response matters.
    upload.abort();

    let (status, headers, body) = parse_response(&received);
    RawAnswer {
        status,
        headers,
        body,
        read_end,
    }
}

/// Split raw HTTP/1.1 response bytes into their status, headers and content.
fn parse_response(received: &[u8]) -> (StatusCode, HeaderMap, Vec<u8>) {
    let separator = received.windows(4).position(|window| window == b"\r\n\r\n").expect("a complete response head");
    let head = str::from_utf8(&received[..separator]).expect("the response head is text");
    let mut lines = head.split("\r\n");
    let status_line = lines.next().expect("a status line");
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .and_then(|code| StatusCode::from_u16(code).ok())
        .expect("a status code");
    let mut headers = HeaderMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').expect("a header line");
        headers.append(
            HeaderName::from_bytes(name.trim().as_bytes()).expect("a header name"),
            HeaderValue::from_str(value.trim()).expect("a header value"),
        );
    }
    let content = &received[separator + 4..];
    let length = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(content.len());
    (status, headers, content[..length.min(content.len())].to_vec())
}

/// Whether `body` is the clause 6.3.2 problem detail naming the limit.
fn is_too_large_problem(body: &[u8]) -> bool {
    let problem: Option<Value> = serde_json::from_slice(body).ok();
    problem.is_some_and(|problem| {
        problem["type"] == "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge"
            && problem["status"] == 413
            && problem["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains(&format!("{MAX_REQUEST_BODY} bytes")))
    })
}

/// Every body-taking operation refuses a payload one byte over the limit with
/// the 413 of clause 6.3.2, as a problem detail that names the limit, and
/// announces that the connection closes after it (RFC 9110 clause 10.1.1).
#[tokio::test]
async fn regression_request_entity_too_large_every_payload_reader_refuses_with_a_problem_detail() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let endpoint = scope.client().endpoint();

    let mut failures = Vec::new();
    for BodyTakingOperation { method, path, content_type } in OPERATIONS {
        let url = endpoint.join(path).expect("a valid operation path");
        let answer = timeout(EXCHANGE_DEADLINE, upload_reading_while_writing(&url, &method, content_type))
            .await
            .expect("the oversized exchange finishes");
        let well_formed = answer.status == StatusCode::PAYLOAD_TOO_LARGE
            && answer.headers.get(CONNECTION).and_then(|value| value.to_str().ok()) == Some("close")
            && is_too_large_problem(&answer.body);
        if !well_formed {
            failures.push(format!(
                "{method} {path}: {} {:?} {}",
                answer.status,
                answer.headers.get(CONNECTION),
                String::from_utf8_lossy(&answer.body)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "oversized payloads not refused as a 413 problem detail that closes the connection:\n{}",
        failures.join("\n")
    );

    scope.finish().await.expect("cleanup failed");
}

/// A hyper-based HTTP/1.1 client, which reports a failed write before a
/// response it has already received, gets the problem detail every time: the
/// connection is closed in stages rather than reset under the upload.
#[tokio::test]
async fn regression_request_entity_too_large_a_hyper_client_receives_the_problem_detail() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let oversized = vec![b' '; MAX_REQUEST_BODY + 1];

    for upload in 1..=HYPER_CLIENT_UPLOADS {
        let response = client
            .raw(Method::POST, "entities")
            .header("content-type", "application/json")
            .body(oversized.clone()) // clone: every request owns its payload
            .send()
            .await;
        assert!(
            response.is_ok(),
            "upload {upload} of {HYPER_CLIENT_UPLOADS} got no answer: {:?}",
            response.as_ref().err()
        );
        let response = response.expect("the broker answered");

        assert_eq!(response.status, StatusCode::PAYLOAD_TOO_LARGE, "upload {upload}");
        assert!(
            is_too_large_problem(&response.body),
            "upload {upload}: {}",
            String::from_utf8_lossy(&response.body)
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Over HTTP/2 the same refusal is the problem detail without a `Connection`
/// header, which RFC 9113 clause 8.2.2 forbids there.
#[tokio::test]
async fn regression_request_entity_too_large_over_http2_is_a_problem_detail_without_connection_header() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let url = scope.client().endpoint().join("entities").expect("a valid operation path");
    let client = reqwest::Client::builder().http2_prior_knowledge().build().expect("an HTTP/2 client");

    let response = client
        .post(url)
        .header("content-type", "application/json")
        .body(vec![b' '; MAX_REQUEST_BODY + 1])
        .send()
        .await
        .expect("the broker answers");

    assert_eq!(response.version(), Version::HTTP_2);
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers().get(CONNECTION), None);
    let body = response.bytes().await.expect("the problem detail");
    assert!(is_too_large_problem(&body), "{}", String::from_utf8_lossy(&body));

    scope.finish().await.expect("cleanup failed");
}

/// After the refusal the broker ends its side of the connection cleanly: the
/// client reads the whole response and then the end of the stream, not a reset.
#[tokio::test]
async fn regression_request_entity_too_large_the_connection_closes_cleanly_after_the_refusal() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let url = scope.client().endpoint().join("jsonldContexts").expect("a valid operation path");

    let answer = timeout(EXCHANGE_DEADLINE, upload_reading_while_writing(&url, &Method::POST, "application/json"))
        .await
        .expect("the oversized exchange finishes");

    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer.read_end, Ok(()));

    scope.finish().await.expect("cleanup failed");
}

/// A payload at the limit is not refused as too large: whatever the operation
/// makes of its content, the size alone is admitted.
#[tokio::test]
async fn regression_request_entity_too_large_a_payload_at_the_limit_is_admitted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let response = client
        .raw(Method::POST, "entities")
        .header("content-type", "application/json")
        .body(vec![b' '; MAX_REQUEST_BODY])
        .send()
        .await
        .expect("the broker answers");

    assert_ne!(response.status, StatusCode::PAYLOAD_TOO_LARGE, "{:?}", String::from_utf8_lossy(&response.body));

    scope.finish().await.expect("cleanup failed");
}
