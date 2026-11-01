//! Downloading and parsing one remote `@context` document.
//!
//! Everything a response must satisfy before its body becomes a context lives
//! here: the media type has to be JSON-shaped, the body has to fit the
//! configured limit, it has to parse, and it has to carry a usable top-level
//! `@context`. The response's cache directives are read on the way past, since
//! clause 6.3.16 makes them part of what was fetched rather than an afterthought
//! the cache tiers each re-derive.
//!
//! What leaves here is the document's `@context` subtree and nothing else.
//! Clause 5.13.2.3 requires that "all extra information located outside of the
//! @context subtree in the referenced object shall be discarded", and a
//! downloaded document is exactly such a referenced object, so the reduction
//! happens where the response becomes a document rather than at each tier that
//! later stores or serves it.

use crate::{
    context_subtree::context_subtree,
    loader::{
        alternate_link::{JsonLdAlternate, json_ld_alternate},
        cache_lifetime::{CacheLifetime, ContextFreshness, response_freshness},
        context_client::ContextClient,
        error::{ContextUnavailable, InvalidContextDocument, LoaderError, LoaderErrorKind},
    },
};
use andromeda_json_literal::JsonLiteral;
use andromeda_outbound_http::endpoint_refusal::EndpointRefusal;
use andromeda_retry::{retry_policy::RetryPolicy, schedule::BackoffSchedule, tokio_retry_delay::TokioRetryDelay};
use headers::HeaderMapExt;
use iri_rs::IriBuf;
use mediatype::MediaTypeBuf;
use reqwest::{Response, header::HeaderMap};
use std::{num::NonZeroU32, str::from_utf8, sync::Arc, time::Duration};
use tracing::warn;

/// A parsed remote context, the freshness its response granted it, and where
/// it was finally read from.
#[derive(Debug)]
pub(crate) struct RemoteContextDocument {
    /// The document's `@context` subtree, as written.
    pub(crate) value: JsonLiteral,
    /// What the response permits a cache to do with the document.
    pub(crate) freshness: ContextFreshness,
    /// The URL the document was finally read from, after redirects and a
    /// JSON-LD alternate: the base every relative reference inside it resolves
    /// against (the JSON-LD 1.1 API's `RemoteDocument.documentUrl`, "the final
    /// URL of the loaded document").
    pub(crate) document_url: IriBuf,
}

/// The media type a loaded remote document reports.
pub(crate) fn ld_json_media_type() -> MediaTypeBuf {
    jsonld_core::LD_JSON_MEDIA_TYPE.into()
}

/// Fetch a context over HTTP and parse it, refusing anything that is not
/// JSON-shaped or is larger than the configured limit.
///
/// A response served as a media type no context is read from is replaced by
/// the one JSON-LD alternate it names (JSON-LD 1.1 section 6.2), fetched once
/// under the same limits; the alternate's own freshness governs the document,
/// and a failure is still reported under `url`, the URL the caller stores the
/// document under. An alternate that is itself not JSON-shaped is not
/// followed further.
///
/// `default_lifetime` is the heuristic expiration time clause 6.3.16 recommends
/// assigning when the origin server states none.
///
/// # Errors
///
/// Returns the [`LoaderError`] naming the step that refused the response: the
/// address policy, the request itself, a non-success status, a media type no context is read from
/// with no single alternate, a body past `max_bytes`, or a document that will
/// not parse as a context.
pub(crate) async fn fetch_and_parse(
    client: &ContextClient,
    url: &IriBuf,
    max_bytes: usize,
    default_lifetime: CacheLifetime,
) -> Result<RemoteContextDocument, LoaderError> {
    let response = successful_response(client, url, url).await?;
    let document_url = final_url(&response, url)?;
    let Some(refusal) = media_type_refusal(response.headers()) else {
        return read_document(response, document_url, url, max_bytes, default_lifetime).await;
    };
    match json_ld_alternate(response.headers(), &document_url) {
        JsonLdAlternate::None => Err(unavailable(url, refusal)),
        JsonLdAlternate::Ambiguous(count) => Err(unavailable(url, ContextUnavailable::AmbiguousAlternates(count))),
        JsonLdAlternate::One(alternate) => {
            let alternate_response = successful_response(client, &alternate, url).await?;
            let alternate_url = final_url(&alternate_response, url)?;
            match media_type_refusal(alternate_response.headers()) {
                Some(alternate_refusal) => Err(unavailable(url, alternate_refusal)),
                None => read_document(alternate_response, alternate_url, url, max_bytes, default_lifetime).await,
            }
        }
    }
}

/// Send the context request for `target` and require a success status,
/// reporting any failure under `reported`.
async fn successful_response(client: &ContextClient, target: &IriBuf, reported: &IriBuf) -> Result<Response, LoaderError> {
    let response = client.get(target).await.map_err(|reason| unavailable(reported, reason))?;
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(LoaderError {
            url: reported.clone(), // clone: the failure is reported under the URL that was requested
            kind: LoaderErrorKind::Http(response.status()),
        })
    }
}

/// The URL a response was finally read from, after any redirects.
fn final_url(response: &Response, reported: &IriBuf) -> Result<IriBuf, LoaderError> {
    IriBuf::new(response.url().as_str().to_owned()).map_err(|error| unavailable(reported, ContextUnavailable::UnaddressableDocument(Arc::new(error))))
}

/// Read, bound and parse the body of a JSON-shaped response read from
/// `document_url`, reporting any failure under `reported`.
async fn read_document(
    response: Response,
    document_url: IriBuf,
    reported: &IriBuf,
    max_bytes: usize,
    default_lifetime: CacheLifetime,
) -> Result<RemoteContextDocument, LoaderError> {
    // Checked twice on purpose: the advertised length rejects an oversized
    // body before it is read, and the measured length catches a server that
    // did not advertise one.
    if let Some(advertised) = response.content_length() {
        // An advertised length wider than the address space saturates: such a
        // body could not be held in memory, so `usize::MAX` refuses it under
        // every configured limit.
        let advertised_bytes = usize::try_from(advertised).unwrap_or(usize::MAX);
        if advertised_bytes > max_bytes {
            return Err(too_large(reported.clone(), advertised_bytes, max_bytes)); // clone: the failure outlives the borrowed URL
        }
    }

    let freshness = response_freshness(response.headers(), default_lifetime);
    let bytes = response
        .bytes()
        .await
        .map_err(|error| unavailable(reported, ContextUnavailable::Request(Arc::new(error))))?;

    if bytes.len() > max_bytes {
        return Err(too_large(reported.clone(), bytes.len(), max_bytes)); // clone: the failure outlives the borrowed URL
    }

    let text = from_utf8(&bytes).map_err(|error| invalid(reported, InvalidContextDocument::NotUtf8(error)))?;
    let document = JsonLiteral::admit(text).map_err(|error| invalid(reported, InvalidContextDocument::NotJson(Arc::new(error))))?;
    let subtree = context_subtree(&document).map_err(|reason| invalid(reported, InvalidContextDocument::NoContext(reason)))?;
    Ok(RemoteContextDocument {
        value: subtree,
        freshness,
        document_url,
    })
}

/// A failure to obtain the context at `url`.
fn unavailable(url: &IriBuf, reason: ContextUnavailable) -> LoaderError {
    LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Unavailable(reason),
    }
}

/// A failure of the document obtained for `url`.
fn invalid(url: &IriBuf, reason: InvalidContextDocument) -> LoaderError {
    LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Invalid(reason),
    }
}

/// How often, and how far apart, a transport-level fetch failure is retried:
/// three attempts in all, 250 ms apart. A context host that drops or refuses
/// a connection under load recovers within a few hundred milliseconds; only a
/// genuinely unreachable source exhausts all attempts.
const TRANSPORT_RETRY: RetryPolicy = RetryPolicy {
    attempts: NonZeroU32::MIN.saturating_add(2),
    schedule: BackoffSchedule::constant(Duration::from_millis(250)),
};

/// [`fetch_and_parse`], retrying only a transport-level failure.
///
/// A dropped or refused connection ([`ContextUnavailable::Request`]) is transient
/// and worth another attempt; every other failure is deterministic -- an HTTP
/// status, an oversized body, or an unparseable document fails identically on a
/// retry -- and is returned at once, filtered by
/// [`is_transient_transport_failure`]. Retrying matters most under
/// `--isolate-tenants`, where the per-tenant context cache makes many fresh
/// tenants cold-fetch the same public context at once and a host occasionally
/// resets one connection, but the same transience arises for any deployment
/// fetching a remote `@context`.
pub(crate) async fn fetch_and_parse_retrying(
    client: &ContextClient,
    url: &IriBuf,
    max_bytes: usize,
    default_lifetime: CacheLifetime,
) -> Result<RemoteContextDocument, LoaderError> {
    // Boxed: the requests that await a context fetch are already deep
    // futures, and holding the retry loop inline takes their layout past the
    // compiler's query depth.
    Box::pin(TRANSPORT_RETRY.run(
        &TokioRetryDelay,
        |error: &LoaderError, retry| {
            let transient = is_transient_transport_failure(error);
            if transient {
                let delay = TRANSPORT_RETRY.schedule.delay(retry);
                warn!(url = %error.url, error = %error.kind, ?delay, "Remote context fetch failed at the transport level; retrying");
            }
            transient
        },
        |_| fetch_and_parse(client, url, max_bytes, default_lifetime),
    ))
    .await
}

/// Whether a loader failure is a transient transport error worth retrying.
///
/// Only a network- or transport-level request failure recovers on a retry, a
/// failed resolution by the address guard included. A refused address, a
/// non-success HTTP status, an oversized body, an unparseable document, a
/// disabled fetch, or a degraded durable cache are all deterministic given the
/// same request, so retrying them only delays the inevitable report.
const fn is_transient_transport_failure(error: &LoaderError) -> bool {
    matches!(
        error.kind,
        LoaderErrorKind::Unavailable(ContextUnavailable::Request(_) | ContextUnavailable::Refused(EndpointRefusal::DnsFailed { .. }))
    )
}

/// Why a response's `Content-Type` names nothing a context can be parsed from,
/// or `None` when it names something one can or states none at all.
///
/// Clause 5.5.6 makes a context that "is not available" `LdContextNotAvailable`
/// and one that "is invalid" `BadRequestData`; a response of the wrong media
/// type yields no document to judge, so it is the former.
fn media_type_refusal(headers: &HeaderMap) -> Option<ContextUnavailable> {
    let content_type = headers.typed_get::<headers::ContentType>()?;
    match content_type.to_string().parse::<MediaTypeBuf>() {
        Ok(media_type) if is_json_like(&media_type) => None,
        Ok(media_type) => Some(ContextUnavailable::UnsupportedMediaType(Box::new(media_type))),
        Err(source) => Some(ContextUnavailable::MalformedMediaType(source)),
    }
}

/// Whether a media type names something a context can be parsed from.
///
/// Wider than the JSON-LD 1.1 API's `LoadDocumentCallback`, which fails a
/// document served as anything but JSON: ETSI GS CIM 009 references only the
/// JSON-LD 1.1 Syntax specification and sets no media type for a fetched
/// `@context` (clauses 5.5.6 and 6.3.16 govern only its availability and its
/// lifetime), while the contexts NGSI-LD data models publish are commonly
/// served as `text/plain` by the hosts they live on. What the body must be is
/// checked after this regardless: UTF-8 JSON with a top-level `@context`
/// (clause 5.13.2.3). A response that states no `Content-Type` at all is not
/// refused either, for the same reason; the caller never asks this then.
fn is_json_like(media_type: &MediaTypeBuf) -> bool {
    let subtype = media_type.subty().as_str();
    subtype == "json"
        || subtype == "ld+json"
        || media_type.suffix().is_some_and(|suffix| suffix.as_str() == "json")
        || (media_type.ty().as_str() == "text" && subtype == "plain")
}

/// The failure reported for a context past the configured size limit.
const fn too_large(url: IriBuf, size: usize, max: usize) -> LoaderError {
    LoaderError {
        url,
        kind: LoaderErrorKind::TooLarge { size, max },
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        context_cache::ContextCacheError,
        context_subtree::{UnusableLocalContext, context_subtree},
        context_tail::ContextTail,
        error::JsonLdError,
        loader::{
            cache_lifetime::CacheLifetime,
            error::{ContextUnavailable, InvalidContextDocument, LoaderError, LoaderErrorKind},
            remote_document::{RemoteContextDocument, fetch_and_parse, is_json_like, is_transient_transport_failure, ld_json_media_type},
            test_origin::{TestOrigin, client, ok, redirect, untyped},
        },
        test_documents::{as_value, literal},
    };
    use andromeda_configuration::{address_rule::AddressRule, schema::private_addresses::PrivateAddresses};
    use andromeda_outbound_http::{address_policy::AddressPolicy, endpoint_refusal::EndpointRefusal};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use iri_rs::IriBuf;
    use mediatype::MediaTypeBuf;
    use reqwest::{Client, StatusCode};
    use serde_json::{Value, json};
    use std::{
        io,
        net::{IpAddr, Ipv4Addr},
        sync::Arc,
    };

    /// A context document naming one term.
    const TERMS: &str = r#"{"@context": {"speed": "https://example.org/speed"}}"#;

    /// Fetch `path` from `origin`.
    async fn fetched(origin: &TestOrigin, path: &str) -> Result<RemoteContextDocument, LoaderError> {
        fetch_and_parse(&client(AddressPolicy::default()), &origin.url(path), 2_000_000, CacheLifetime::DEFAULT).await
    }

    /// Fetch `path` from `origin` under `addresses`.
    async fn fetched_under(addresses: AddressPolicy, origin: &TestOrigin, path: &str) -> Result<RemoteContextDocument, LoaderError> {
        fetch_and_parse(&client(addresses), &origin.url(path), 2_000_000, CacheLifetime::DEFAULT).await
    }

    /// Private addresses refused, with the origin's own loopback address
    /// exempt and every other loopback address refused.
    fn refusing_all_but_the_origin() -> AddressPolicy {
        let origin: AddressRule = "127.0.0.1".parse().expect("a well-formed rule");
        AddressPolicy::new(PrivateAddresses::Refused, vec![origin], Vec::new())
    }

    /// Whether `error` is a refusal of `refused`.
    fn is_refusal_of(error: &LoaderError, refused: Ipv4Addr) -> bool {
        matches!(
            &error.kind,
            LoaderErrorKind::Unavailable(ContextUnavailable::Refused(EndpointRefusal::Blocked { ip })) if *ip == IpAddr::V4(refused)
        )
    }

    /// A loopback address other than the origin's, on the origin's port.
    const ELSEWHERE: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);

    /// The document as JSON.
    fn json_of(document: &RemoteContextDocument) -> Value {
        as_value(&document.value)
    }

    /// JSON-LD 1.1 section 6.2: an HTML page naming one JSON-LD alternate is
    /// replaced by that alternate, fetched once, and the document's base is
    /// where the alternate was read from.
    #[tokio::test]
    async fn a_non_json_response_is_replaced_by_the_json_ld_alternate_it_names() {
        let origin = TestOrigin::start().await;
        origin
            .stage(
                "/page",
                ok(
                    "text/html",
                    "<html></html>",
                    &[r#"</docs/context.jsonld>; rel="alternate"; type="application/ld+json""#],
                ),
            )
            .await;
        origin.stage("/docs/context.jsonld", ok("application/ld+json", TERMS, &[])).await;

        let document = fetched(&origin, "/page").await.expect("the alternate is followed");

        pretty_assertions::assert_eq!(json_of(&document), json!({"@context": {"speed": "https://example.org/speed"}}));
        pretty_assertions::assert_eq!(document.document_url, origin.url("/docs/context.jsonld"));
        pretty_assertions::assert_eq!(origin.requests("/docs/context.jsonld").await, 1);
    }

    /// Only one hop is followed: an alternate that is itself not JSON-shaped
    /// is refused as a context that could not be obtained, even when it names
    /// another alternate.
    #[tokio::test]
    async fn an_alternate_that_is_not_json_is_not_followed_again() {
        let origin = TestOrigin::start().await;
        origin
            .stage(
                "/page",
                ok("text/html", "<html></html>", &[r#"</second>; rel="alternate"; type="application/ld+json""#]),
            )
            .await;
        origin
            .stage(
                "/second",
                ok("text/html", "<html></html>", &[r#"</third>; rel="alternate"; type="application/ld+json""#]),
            )
            .await;
        origin.stage("/third", ok("application/ld+json", TERMS, &[])).await;

        let refused = fetched(&origin, "/page").await.expect_err("a second hop is not followed");

        assert!(
            matches!(refused.kind, LoaderErrorKind::Unavailable(ContextUnavailable::UnsupportedMediaType(_))),
            "{refused}"
        );
        pretty_assertions::assert_eq!(refused.url, origin.url("/page"));
        pretty_assertions::assert_eq!(origin.requests("/third").await, 0);
    }

    #[tokio::test]
    async fn two_json_ld_alternates_are_refused() {
        let origin = TestOrigin::start().await;
        origin
            .stage(
                "/page",
                ok(
                    "text/html",
                    "<html></html>",
                    &[
                        r#"</a.jsonld>; rel="alternate"; type="application/ld+json""#,
                        r#"</b.jsonld>; rel="alternate"; type="application/ld+json""#,
                    ],
                ),
            )
            .await;

        let refused = fetched(&origin, "/page").await.expect_err("two alternates state no context");

        assert!(
            matches!(refused.kind, LoaderErrorKind::Unavailable(ContextUnavailable::AmbiguousAlternates(2))),
            "{refused}"
        );
    }

    /// Clause 5.5.6: a response of a media type no context is read from, with
    /// no alternate, yielded no document, so it is `LdContextNotAvailable`.
    #[tokio::test]
    async fn a_refused_media_type_without_an_alternate_is_a_context_not_available() {
        let origin = TestOrigin::start().await;
        origin.stage("/binary", ok("application/octet-stream", TERMS, &[])).await;

        let refused = fetched(&origin, "/binary").await.expect_err("octet-stream is not a context");

        assert!(JsonLdError::from(&refused).is_context_not_available(), "{refused}");
    }

    /// `text/plain` and a response with no `Content-Type` stay accepted.
    #[tokio::test]
    async fn text_plain_and_a_missing_content_type_are_read_as_json() {
        let origin = TestOrigin::start().await;
        origin.stage("/plain", ok("text/plain", TERMS, &[])).await;
        origin.stage("/bare", untyped(TERMS)).await;

        assert!(fetched(&origin, "/plain").await.is_ok());
        assert!(fetched(&origin, "/bare").await.is_ok());
    }

    /// The JSON-LD 1.1 API's `RemoteDocument.documentUrl` is "the final URL
    /// of the loaded document": a relative reference inside a redirected
    /// document resolves against where it was read from, not where it was
    /// requested.
    #[tokio::test]
    async fn a_relative_reference_in_a_redirected_document_resolves_against_the_final_url() {
        let origin = TestOrigin::start().await;
        origin.stage("/moved.jsonld", redirect("/contexts/v2/wrapper.jsonld")).await;
        origin
            .stage("/contexts/v2/wrapper.jsonld", ok("application/ld+json", r#"{"@context": "terms.jsonld"}"#, &[]))
            .await;

        let document = fetched(&origin, "/moved.jsonld").await.expect("the redirect is followed");

        pretty_assertions::assert_eq!(document.document_url, origin.url("/contexts/v2/wrapper.jsonld"));
        pretty_assertions::assert_eq!(
            ContextTail::of_document(document.value.piece()).resolved_against(&document.document_url),
            ContextTail::Reference(origin.url("/contexts/v2/terms.jsonld"))
        );
    }

    /// A loader error under a fixed URL, for the retry-classification test.
    fn error(kind: LoaderErrorKind) -> LoaderError {
        LoaderError {
            url: IriBuf::new("https://example.com/context.jsonld".to_owned()).expect("valid URL"),
            kind,
        }
    }

    #[tokio::test]
    async fn a_context_url_in_refused_address_space_is_refused_without_a_request() {
        let origin = TestOrigin::start().await;
        origin.stage("/context.jsonld", ok("application/ld+json", TERMS, &[])).await;
        let refusing = AddressPolicy::new(PrivateAddresses::Refused, Vec::new(), Vec::new());

        let error = fetched_under(refusing, &origin, "/context.jsonld").await.expect_err("loopback is refused");

        assert!(is_refusal_of(&error, Ipv4Addr::LOCALHOST), "{error:?}");
        assert_eq!(origin.requests("/context.jsonld").await, 0);
    }

    #[tokio::test]
    async fn a_context_url_on_a_denied_address_is_refused_though_private_addresses_are_allowed() {
        let origin = TestOrigin::start().await;
        origin.stage("/context.jsonld", ok("application/ld+json", TERMS, &[])).await;
        let denied: AddressRule = "127.0.0.1".parse().expect("a well-formed rule");
        let denying = AddressPolicy::new(PrivateAddresses::Allowed, Vec::new(), vec![denied]);

        let error = fetched_under(denying, &origin, "/context.jsonld")
            .await
            .expect_err("a denied address is refused");

        assert!(is_refusal_of(&error, Ipv4Addr::LOCALHOST), "{error:?}");
        assert_eq!(origin.requests("/context.jsonld").await, 0);
    }

    #[tokio::test]
    async fn a_context_url_on_an_allowed_address_is_downloaded_under_a_refusing_policy() {
        let origin = TestOrigin::start().await;
        origin.stage("/context.jsonld", ok("application/ld+json", TERMS, &[])).await;

        let document = fetched_under(refusing_all_but_the_origin(), &origin, "/context.jsonld")
            .await
            .expect("an allowed address is downloaded");

        assert_eq!(json_of(&document), json!({"@context": {"speed": "https://example.org/speed"}}));
    }

    #[tokio::test]
    async fn a_redirect_into_refused_address_space_is_refused() {
        let origin = TestOrigin::start().await;
        let elsewhere = format!("http://{ELSEWHERE}:{}/context.jsonld", origin.port());
        origin.stage("/moved", redirect(&elsewhere)).await;

        let error = fetched_under(refusing_all_but_the_origin(), &origin, "/moved")
            .await
            .expect_err("the redirect target is refused");

        assert!(is_refusal_of(&error, ELSEWHERE), "{error:?}");
        assert_eq!(error.url, origin.url("/moved"), "the failure is reported under the URL that was asked for");
    }

    #[tokio::test]
    async fn a_json_ld_alternate_in_refused_address_space_is_refused() {
        let origin = TestOrigin::start().await;
        let alternate = format!(
            r#"<http://{ELSEWHERE}:{}/context.jsonld>; rel="alternate"; type="application/ld+json""#,
            origin.port()
        );
        origin.stage("/page", ok("text/html", "<html></html>", &[alternate.as_str()])).await;

        let error = fetched_under(refusing_all_but_the_origin(), &origin, "/page")
            .await
            .expect_err("the alternate is refused");

        assert!(is_refusal_of(&error, ELSEWHERE), "{error:?}");
        assert_eq!(error.url, origin.url("/page"), "the failure is reported under the URL that was asked for");
    }

    #[test]
    fn a_refused_address_is_not_retried_and_a_failed_guard_resolution_is() {
        let blocked = EndpointRefusal::Blocked {
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        };
        assert!(!is_transient_transport_failure(&error(LoaderErrorKind::Unavailable(
            ContextUnavailable::Refused(blocked)
        ))));
        let unresolved = EndpointRefusal::DnsFailed {
            source: Arc::new(io::Error::other("temporary failure in name resolution")),
        };
        assert!(is_transient_transport_failure(&error(LoaderErrorKind::Unavailable(
            ContextUnavailable::Refused(unresolved)
        ))));
    }

    #[test]
    fn only_a_transport_failure_is_retried() {
        assert!(
            is_transient_transport_failure(&error(LoaderErrorKind::Unavailable(ContextUnavailable::Request(Arc::new(
                Client::new().get("not a url").build().expect_err("a malformed URL fails the request builder")
            ))))),
            "a dropped or refused connection is transient and retried"
        );
        for deterministic in [
            LoaderErrorKind::Http(StatusCode::NOT_FOUND),
            LoaderErrorKind::TooLarge { size: 10, max: 5 },
            LoaderErrorKind::Invalid(InvalidContextDocument::NoContext(UnusableLocalContext::NoContextMember)),
            LoaderErrorKind::FetchDisabled,
            LoaderErrorKind::Unavailable(ContextUnavailable::Cache(ContextCacheError::Read(PersistenceFailure::Unavailable))),
        ] {
            assert!(!is_transient_transport_failure(&error(deterministic)), "a deterministic failure is not retried");
        }
    }

    /// Clause 5.13.2.3: a downloaded `@context` is a referenced object, so
    /// "all extra information located outside of the @context subtree" is gone
    /// before the document reaches either cache tier; otherwise the extras
    /// would be stored whole and served back (clause 5.13.4).
    #[test]
    fn a_downloaded_document_is_reduced_to_its_context_subtree() {
        let downloaded = json!({
            "@context": { "Device": "https://example.org/Device" },
            "id": "urn:ngsi-ld:Context:from-the-origin-server",
            "unrelated": [1, 2, 3]
        });

        pretty_assertions::assert_eq!(
            context_subtree(&literal(&downloaded))
                .map(|reduced| as_value(&reduced))
                .expect("a local context"),
            json!({ "@context": { "Device": "https://example.org/Device" } })
        );
    }

    #[test]
    fn only_json_shaped_content_types_are_accepted_for_a_context() {
        for accepted in ["application/json", "application/ld+json", "application/vnd.example+json", "text/plain"] {
            let media_type: MediaTypeBuf = accepted.parse().expect("a media type");
            assert!(is_json_like(&media_type), "{accepted} must be accepted");
        }
        for rejected in ["text/html", "application/octet-stream"] {
            let media_type: MediaTypeBuf = rejected.parse().expect("a media type");
            assert!(!is_json_like(&media_type), "{rejected} must be refused");
        }
    }

    #[test]
    fn a_loaded_document_reports_the_json_ld_media_type() {
        pretty_assertions::assert_eq!(ld_json_media_type().to_string(), "application/ld+json");
    }
}
