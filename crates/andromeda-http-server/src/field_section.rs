//! The size of an HTTP/1.x request's header section, measured as HTTP/2 and
//! HTTP/3 measure theirs, and the refusal of a section over the limit.
//!
//! HTTP/2 and HTTP/3 refuse an oversized section in the protocol library,
//! against the same limit, before a request reaches the service. HTTP/1.1 has
//! no such measure on the wire, so its requests are measured here, as the
//! pseudo-header fields and fields HTTP/2 would carry them as: the method, the
//! listener's scheme, the authority (from the target or from `Host`, which is
//! then not counted again) and the target's path and query, then every other
//! field as it arrived.

use andromeda_configuration::{header_section_limit::HeaderSectionLimit, schema::listener_transport::ListenerTransport};
use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode, Version, header::HOST, uri::Authority},
    middleware::Next,
    response::{IntoResponse, Response},
};
use headers::{Connection, HeaderMapExt};

/// The octets RFC 9113 section 6.5.2 and RFC 9114 section 4.2.2 add to every
/// field, beyond its name and value.
const FIELD_OVERHEAD: usize = 32;

/// The smallest field a section can carry: a one-octet name and an empty value.
pub(crate) const SMALLEST_FIELD: usize = FIELD_OVERHEAD + 1;

/// What a listener refuses of a request's header section.
#[derive(Clone, Copy, Debug)]
pub struct HeaderSectionPolicy {
    /// The largest section accepted.
    pub limit: HeaderSectionLimit,
    /// The listener's transport, which decides the `:scheme` an HTTP/1.x
    /// request is measured with.
    pub transport: ListenerTransport,
}

/// The size of `request`'s header section as HTTP/2 would carry it.
pub(crate) fn field_section_size(request: &Request, transport: ListenerTransport) -> usize {
    let scheme = match transport {
        ListenerTransport::Plaintext => "http",
        ListenerTransport::Tls => "https",
    };
    let uri = request.uri();
    let method = field_size(":method", request.method().as_str().len());
    // An authority-form target (CONNECT) has no path, and RFC 9113 section
    // 8.5 carries it without `:scheme` and `:path`.
    let target = uri
        .path_and_query()
        .map_or(0, |path| field_size(":scheme", scheme.len()) + field_size(":path", path.as_str().len()));
    let authority = uri
        .authority()
        .map(Authority::as_str)
        .map(str::len)
        .or_else(|| request.headers().get(HOST).map(HeaderValue::len))
        .map_or(0, |length| field_size(":authority", length));
    let fields: usize = request
        .headers()
        .iter()
        .filter(|(name, _)| *name != HOST)
        .map(|(name, value)| field_size(name.as_str(), value.len()))
        .sum();
    method + target + authority + fields
}

/// The size of one field named `name` with a value of `value` octets.
const fn field_size(name: &str, value: usize) -> usize {
    name.len() + value + FIELD_OVERHEAD
}

/// Answer an HTTP/1.x request whose header section is over the limit with a
/// bare 431, as the HTTP/2 and HTTP/3 libraries answer theirs.
///
/// The request's content is never read, and over HTTP/1.1 closing the
/// connection is the only way to stop it, so the refusal announces the close.
pub(crate) async fn refuse_oversized_http1_section(State(policy): State<HeaderSectionPolicy>, request: Request, next: Next) -> Response {
    if request.version() < Version::HTTP_2 && field_section_size(&request, policy.transport) > usize::from(policy.limit.octets()) {
        let mut response = StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE.into_response();
        response.headers_mut().typed_insert(Connection::close());
        response
    } else {
        next.run(request).await
    }
}

#[cfg(test)]
mod tests {
    use crate::field_section::{HeaderSectionPolicy, field_section_size, refuse_oversized_http1_section};
    use andromeda_configuration::{header_section_limit::HeaderSectionLimit, schema::listener_transport::ListenerTransport};
    use axum::{
        Router,
        body::Body,
        extract::Request,
        http::{StatusCode, Version, header::CONNECTION},
        middleware::from_fn_with_state,
        routing::get,
    };
    use pretty_assertions::assert_eq;
    use std::error::Error;
    use tower::ServiceExt;

    fn origin_form_request() -> Result<Request, Box<dyn Error>> {
        Ok(Request::get("/a?b").header("host", "h").header("accept", "x").body(Body::empty())?)
    }

    #[test]
    fn the_measure_counts_each_pseudo_field_and_field_at_name_value_and_32() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            field_section_size(&origin_form_request()?, ListenerTransport::Plaintext),
            (7 + 3 + 32) + (7 + 4 + 32) + (10 + 1 + 32) + (5 + 4 + 32) + (6 + 1 + 32)
        );
        Ok(())
    }

    #[test]
    fn a_tls_listener_counts_https_as_the_scheme() -> Result<(), Box<dyn Error>> {
        let request = origin_form_request()?;
        assert_eq!(
            field_section_size(&request, ListenerTransport::Tls),
            field_section_size(&request, ListenerTransport::Plaintext) + 1
        );
        Ok(())
    }

    #[test]
    fn host_is_counted_once_as_the_authority() -> Result<(), Box<dyn Error>> {
        let without_host = Request::get("/a?b").header("accept", "x").body(Body::empty())?;
        assert_eq!(
            field_section_size(&origin_form_request()?, ListenerTransport::Plaintext),
            field_section_size(&without_host, ListenerTransport::Plaintext) + (10 + 1 + 32)
        );
        Ok(())
    }

    #[test]
    fn an_absolute_form_target_takes_its_authority_from_the_uri() -> Result<(), Box<dyn Error>> {
        let request = Request::get("http://broker.example/a?b").header("host", "h").body(Body::empty())?;
        assert_eq!(
            field_section_size(&request, ListenerTransport::Plaintext),
            (7 + 3 + 32) + (7 + 4 + 32) + (10 + 14 + 32) + (5 + 4 + 32)
        );
        Ok(())
    }

    #[test]
    fn an_authority_form_target_carries_no_scheme_and_no_path() -> Result<(), Box<dyn Error>> {
        let request = Request::connect("broker.example:443").body(Body::empty())?;
        assert_eq!(field_section_size(&request, ListenerTransport::Plaintext), (7 + 7 + 32) + (10 + 18 + 32));
        Ok(())
    }

    /// Serve a request carrying a field of `value_length` octets, at `version`,
    /// under an 8 KiB limit.
    async fn answered(version: Version, value_length: usize) -> Result<(StatusCode, Option<String>), Box<dyn Error>> {
        let policy = HeaderSectionPolicy {
            limit: HeaderSectionLimit::MINIMUM,
            transport: ListenerTransport::Plaintext,
        };
        let router = Router::new()
            .route("/", get(|| async { "served" }))
            .layer(from_fn_with_state(policy, refuse_oversized_http1_section));
        let request = Request::get("/")
            .version(version)
            .header("host", "h")
            .header("x", "v".repeat(value_length))
            .body(Body::empty())?;
        let response = router.oneshot(request).await?;
        let connection = response.headers().get(CONNECTION).map(|value| value.to_str().map(str::to_owned)).transpose()?;
        Ok((response.status(), connection))
    }

    /// The value length that fills the 8 KiB section exactly: `:method GET`,
    /// `:scheme http`, `:path /`, `:authority h` and the one field `x`.
    const FILLING_VALUE: usize = 8192 - (7 + 3 + 32) - (7 + 4 + 32) - (5 + 1 + 32) - (10 + 1 + 32) - (1 + 32);

    #[tokio::test]
    async fn an_http1_section_at_the_limit_is_served() -> Result<(), Box<dyn Error>> {
        assert_eq!(answered(Version::HTTP_11, FILLING_VALUE).await?, (StatusCode::OK, None));
        Ok(())
    }

    #[tokio::test]
    async fn an_http1_section_one_octet_over_the_limit_is_refused_431_with_the_close_announced() -> Result<(), Box<dyn Error>> {
        assert_eq!(
            answered(Version::HTTP_11, FILLING_VALUE + 1).await?,
            (StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE, Some("close".to_owned()))
        );
        Ok(())
    }

    #[tokio::test]
    async fn an_http2_request_is_left_to_its_protocol_limit() -> Result<(), Box<dyn Error>> {
        assert_eq!(answered(Version::HTTP_2, FILLING_VALUE + 1).await?, (StatusCode::OK, None));
        Ok(())
    }
}
