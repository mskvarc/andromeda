//! `Accept-Language` read as the clause 4.15 language filter.
//!
//! NGSI-LD carries the language filter in the `lang` parameter only, and RFC
//! 9110 section 12.5.4 lets a server ignore `Accept-Language`, which is what a
//! conformant broker does: browsers send the header on every request, and a
//! request without `lang` is answered with whole `languageMap`s. Under the
//! `accept_language` extension the operations that take `lang` read the header
//! in its place. `negotiate_accept_language` decodes it once, before the
//! handler, and keeps the filter as an `AcceptedLanguage` request extension
//! for [`crate::language`] to hand over when neither `lang` nor
//! `NGSILD-Language` names one.

use crate::addressed_uri::addressed_uri;
use andromeda_http_headers::language::{LanguageFilter, LanguageFilterError};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use axum::{
    extract::Request,
    http::{
        HeaderValue,
        header::{ACCEPT_LANGUAGE, VARY},
    },
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::MethodRouter,
};
use thiserror::Error;

/// The `Vary` member a response chosen by `Accept-Language` names.
static VARY_ON_LANGUAGE: HeaderValue = HeaderValue::from_static("accept-language");

/// Whether an operation that takes `lang` reads `Accept-Language` in its
/// absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptLanguageReading {
    /// The header stands in for an absent `lang`, and the response varies on it.
    Honoured,
    /// The header is ignored, as a conformant broker ignores it.
    Ignored,
}

impl AcceptLanguageReading {
    /// The route of an operation that takes `lang`, with `Accept-Language`
    /// read before its handler when the header is honoured.
    pub fn negotiate<S>(self, route: MethodRouter<S>) -> MethodRouter<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        match self {
            AcceptLanguageReading::Honoured => route.layer(from_fn(negotiate_accept_language)),
            AcceptLanguageReading::Ignored => route,
        }
    }
}

/// The language filter a request's `Accept-Language` header named.
#[derive(Debug, Clone)]
pub(crate) struct AcceptedLanguage(pub(crate) LanguageFilter);

/// An `Accept-Language` header that is not a language preference list.
#[derive(Debug, Error)]
#[error("the accept-language header is not a language preference list: {0}")]
struct InvalidAcceptLanguage(#[source] LanguageFilterError);

/// Decode `Accept-Language` into an [`AcceptedLanguage`] request extension, and
/// mark the response as varying on the header (RFC 9110 section 12.5.5).
///
/// A header naming no language leaves the request without a filter. A header
/// that is not a preference list is refused as `BadRequestData`, as an
/// unparsable `lang` is.
async fn negotiate_accept_language(request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    let mut response = match LanguageFilter::from_header_values(parts.headers.get_all(ACCEPT_LANGUAGE)) {
        Ok(filter) => {
            if let Some(filter) = filter {
                parts.extensions.insert(AcceptedLanguage(filter));
            }
            next.run(Request::from_parts(parts, body)).await
        }
        Err(error) => ExtractorRejection::bad_request_data(InvalidAcceptLanguage(error).to_string(), addressed_uri(&parts)).into_response(),
    };
    response.headers_mut().append(VARY, VARY_ON_LANGUAGE.clone()); // clone: a header value is a cheap shared byte buffer
    response
}

#[cfg(test)]
mod tests {
    use crate::{accept_language::AcceptLanguageReading, language::LanguageNegotiation};
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{HeaderValue, Request, StatusCode, header::VARY},
        response::Response,
        routing::get,
    };
    use tower::ServiceExt;

    /// The language list the handler was handed, or `none`.
    async fn echo_language(LanguageNegotiation(filter): LanguageNegotiation) -> String {
        filter.map_or_else(
            || "none".to_owned(),
            |filter| filter.to_header_value().expect("a header value").to_str().expect("text").to_owned(),
        )
    }

    async fn send(reading: AcceptLanguageReading, headers: &[(&'static str, &'static str)]) -> Response {
        let router = Router::new().route("/entities", reading.negotiate(get(echo_language)));
        let request = headers
            .iter()
            .fold(Request::builder().uri("/entities"), |builder, (name, value)| builder.header(*name, *value))
            .body(Body::empty())
            .expect("a request");
        router.oneshot(request).await.expect("a response")
    }

    async fn body_text(response: Response) -> String {
        String::from_utf8(to_bytes(response.into_body(), usize::MAX).await.expect("a body").to_vec()).expect("utf-8")
    }

    fn varies_on_language(response: &Response) -> bool {
        response
            .headers()
            .get_all(VARY)
            .iter()
            .any(|value| value == HeaderValue::from_static("accept-language"))
    }

    #[tokio::test]
    async fn an_honoured_header_reaches_the_handler_as_the_language_filter() {
        let response = send(AcceptLanguageReading::Honoured, &[("accept-language", "fr-CH, fr;q=0.9, en;q=0.8")]).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        assert!(varies_on_language(&response));
        pretty_assertions::assert_eq!(body_text(response).await, "fr-CH, fr; q=0.9, en; q=0.8");
    }

    /// RFC 9110 section 12.5.5: the selection depends on the header whether or
    /// not this request carried it, so a cache must key on it either way.
    #[tokio::test]
    async fn a_response_varies_on_the_header_when_the_request_carries_none() {
        let response = send(AcceptLanguageReading::Honoured, &[]).await;
        assert!(varies_on_language(&response));
        pretty_assertions::assert_eq!(body_text(response).await, "none");
    }

    #[tokio::test]
    async fn a_header_naming_no_language_leaves_the_request_unfiltered() {
        let response = send(AcceptLanguageReading::Honoured, &[("accept-language", " , ")]).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        pretty_assertions::assert_eq!(body_text(response).await, "none");
    }

    #[tokio::test]
    async fn an_unparsable_header_is_bad_request_data() {
        let response = send(AcceptLanguageReading::Honoured, &[("accept-language", "en_US")]).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(varies_on_language(&response));
        let body = body_text(response).await;
        assert!(body.contains("https://uri.etsi.org/ngsi-ld/errors/BadRequestData"), "{body}");
        assert!(body.contains("the accept-language header is not a language preference list"), "{body}");
    }

    #[tokio::test]
    async fn the_ngsild_language_header_outranks_accept_language() {
        let response = send(AcceptLanguageReading::Honoured, &[("accept-language", "fr"), ("ngsild-language", "de")]).await;
        pretty_assertions::assert_eq!(body_text(response).await, "de");
    }

    #[tokio::test]
    async fn an_ignored_header_neither_filters_nor_varies_the_response() {
        let response = send(AcceptLanguageReading::Ignored, &[("accept-language", "en_US")]).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        assert!(!varies_on_language(&response));
        pretty_assertions::assert_eq!(body_text(response).await, "none");
    }
}
