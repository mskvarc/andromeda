//! Keeping entity tags and `Vary` honest across response compression.
//!
//! RFC 7232 section 2.3.3: "a strong entity-tag for a content-encoded
//! representation has to be distinct from the entity tag of an unencoded
//! representation". The compression layer content-encodes a response after the
//! handler computed its `ETag` over the unencoded bytes and leaves the tag as
//! it was, so a gzip body would carry the strong tag of the identity body. A
//! strong tag on every response the layer encoded is therefore rewritten to its
//! weak form (`W/"..."`): `If-None-Match` compares weakly (RFC 7232 section
//! 3.2), so the weak tag still revalidates, and a strong comparison no longer
//! mistakes two different byte sequences for one.
//!
//! The layer never encodes a body-less 304, yet RFC 7232 section 4.1 has a 304
//! carry the `ETag` and `Vary` the 200 would have carried. The layer names
//! `accept-encoding` in the `Vary` of every response it would compress,
//! whatever the request's `Accept-Encoding`, so every 304 names it too: the 304
//! has no body to tell a compressible 200 from another, and a `Vary` member the
//! 200 lacked only narrows which requests a cache reuses the response for. A
//! 304 answering a request whose `Accept-Encoding` selects a coding the layer
//! applies stands for the encoded 200, so it also gets that response's weak
//! tag.

use andromeda_configuration::schema::content_coding::ContentCoding;
use axum::{
    http::{
        HeaderMap,
        HeaderValue,
        StatusCode,
        header::{ACCEPT_ENCODING, CONTENT_ENCODING, ETAG, VARY},
    },
    response::Response,
};
use enumset::EnumSet;

/// The prefix that marks an entity tag weak (RFC 7232 section 2.3).
const WEAK_PREFIX: &[u8] = b"W/";

/// The content coding that leaves a representation unencoded.
const IDENTITY_CODING: &[u8] = b"identity";

/// The `Vary` member a response selected by `Accept-Encoding` names.
static VARY_ON_ENCODING: HeaderValue = HeaderValue::from_static("accept-encoding");

/// The codings the compression layer applies, and the header rewrite that
/// keeps a response's validators true to them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EncodedEntityTags {
    /// The codings the compression layer is configured with.
    codings: EnumSet<ContentCoding>,
}

impl EncodedEntityTags {
    /// The rewrite for a compression layer applying `codings`.
    pub(crate) const fn new(codings: EnumSet<ContentCoding>) -> EncodedEntityTags {
        EncodedEntityTags { codings }
    }

    /// Rewrite `response`, answering a request with `request_headers`, once the
    /// compression layer has seen it.
    pub(crate) fn revalidated(self, request_headers: &HeaderMap, mut response: Response) -> Response {
        let not_modified = response.status() == StatusCode::NOT_MODIFIED;
        let stands_for_encoded = not_modified && self.selects_coding(request_headers);
        let headers = response.headers_mut();
        if not_modified {
            vary_on_encoding(headers);
        }
        if stands_for_encoded || is_content_encoded(headers) {
            weaken(headers);
        }
        response
    }

    /// Whether the request's `Accept-Encoding` selects a coding the layer
    /// applies: one it lists, or `*`, with a non-zero quality (RFC 9110
    /// section 12.5.3).
    fn selects_coding(self, request_headers: &HeaderMap) -> bool {
        request_headers
            .get_all(ACCEPT_ENCODING)
            .iter()
            .flat_map(|value| value.as_bytes().split(|byte| *byte == b','))
            .filter(|member| is_acceptable(member))
            .any(|member| {
                let coding = member.split(|byte| *byte == b';').next().unwrap_or_default().trim_ascii();
                if coding == b"*" {
                    return !self.codings.is_empty();
                }
                wire_coding(coding).is_some_and(|coding| self.codings.contains(coding))
            })
    }
}

/// Whether an `Accept-Encoding` member carries a quality above zero; a member
/// without one has the default quality 1.
fn is_acceptable(member: &[u8]) -> bool {
    member
        .split(|byte| *byte == b';')
        .skip(1)
        .map(<[u8]>::trim_ascii)
        .find_map(|parameter| parameter.strip_prefix(b"q=").or_else(|| parameter.strip_prefix(b"Q=")))
        .is_none_or(|quality| quality.iter().any(|digit| (b'1'..=b'9').contains(digit)))
}

/// The coding an `Accept-Encoding` token names, by its RFC 9110 section 8.4.1
/// registry spelling.
fn wire_coding(token: &[u8]) -> Option<ContentCoding> {
    [
        (b"gzip" as &[u8], ContentCoding::Gzip),
        (b"br", ContentCoding::Brotli),
        (b"deflate", ContentCoding::Deflate),
        (b"zstd", ContentCoding::Zstd),
    ]
    .into_iter()
    .find_map(|(spelling, coding)| token.eq_ignore_ascii_case(spelling).then_some(coding))
}

/// Rewrite a strong `ETag` to its weak form.
///
/// A tag that cannot be restated weak (it could only fail on bytes a header
/// value cannot hold, which a tag already in the map never has) is removed
/// rather than left strong: no tag is better than one that misstates which
/// bytes it validates.
fn weaken(headers: &mut HeaderMap) {
    let Some(strong) = headers.get(ETAG).filter(|tag| tag.as_bytes().first() == Some(&b'"')) else {
        return;
    };
    let weak = [WEAK_PREFIX, strong.as_bytes()].concat();
    match HeaderValue::from_bytes(&weak) {
        Ok(weak) => {
            headers.insert(ETAG, weak);
        }
        Err(_unrepresentable) => {
            headers.remove(ETAG);
        }
    }
}

/// Add `accept-encoding` to the response's `Vary` unless it is already named.
fn vary_on_encoding(headers: &mut HeaderMap) {
    let named = headers
        .get_all(VARY)
        .iter()
        .flat_map(|value| value.as_bytes().split(|byte| *byte == b','))
        .any(|member| member.trim_ascii().eq_ignore_ascii_case(VARY_ON_ENCODING.as_bytes()));
    if !named {
        headers.append(VARY, VARY_ON_ENCODING.clone());
    }
}

/// Whether the response names a content coding other than `identity`, read
/// from the header's bytes so an opaque value still counts as a coding.
fn is_content_encoded(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_ENCODING)
        .is_some_and(|coding| !coding.as_bytes().trim_ascii().eq_ignore_ascii_case(IDENTITY_CODING))
}

#[cfg(test)]
mod tests {
    use crate::stack::encoded_entity_tag::EncodedEntityTags;
    use andromeda_configuration::schema::content_coding::ContentCoding;
    use axum::{
        http::{
            HeaderMap,
            HeaderValue,
            StatusCode,
            header::{ACCEPT_ENCODING, CONTENT_ENCODING, ETAG, VARY},
        },
        response::{IntoResponse, Response},
    };
    use enumset::EnumSet;

    fn gzip_only() -> EncodedEntityTags {
        EncodedEntityTags::new(EnumSet::only(ContentCoding::Gzip))
    }

    fn response(status: StatusCode, coding: Option<&'static [u8]>, tag: &'static str) -> Response {
        let mut response = status.into_response();
        if let Some(coding) = coding {
            response
                .headers_mut()
                .insert(CONTENT_ENCODING, HeaderValue::from_bytes(coding).expect("a header value"));
        }
        response.headers_mut().insert(ETAG, HeaderValue::from_static(tag));
        response
    }

    fn accepting(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT_ENCODING, HeaderValue::from_static(value));
        headers
    }

    fn tag(response: &Response) -> Option<&HeaderValue> {
        response.headers().get(ETAG)
    }

    #[test]
    fn a_gzip_encoded_response_carries_the_weak_form_of_its_tag() {
        let encoded = gzip_only().revalidated(&accepting("gzip"), response(StatusCode::OK, Some(b"gzip"), "\"x\""));
        pretty_assertions::assert_eq!(tag(&encoded), Some(&HeaderValue::from_static("W/\"x\"")));
    }

    #[test]
    fn an_unencoded_response_keeps_its_strong_tag() {
        for coding in [None, Some(b"identity" as &[u8]), Some(b" IDENTITY ")] {
            let unencoded = gzip_only().revalidated(&HeaderMap::new(), response(StatusCode::OK, coding, "\"x\""));
            pretty_assertions::assert_eq!(tag(&unencoded), Some(&HeaderValue::from_static("\"x\"")), "{coding:?}");
        }
    }

    #[test]
    fn a_weak_tag_is_left_as_it_is() {
        let encoded = gzip_only().revalidated(&HeaderMap::new(), response(StatusCode::OK, Some(b"br"), "W/\"x\""));
        pretty_assertions::assert_eq!(tag(&encoded), Some(&HeaderValue::from_static("W/\"x\"")));
    }

    /// A `Content-Encoding` that is not visible ASCII still names a coding, so
    /// the tag is weakened rather than the header being read as absent.
    #[test]
    fn an_opaque_content_coding_still_weakens_the_tag() {
        let encoded = gzip_only().revalidated(&HeaderMap::new(), response(StatusCode::OK, Some(b"g\xe9zip"), "\"x\""));
        pretty_assertions::assert_eq!(tag(&encoded), Some(&HeaderValue::from_static("W/\"x\"")));
    }

    /// RFC 7232 section 4.1: a 304 standing for an encoded 200 carries that
    /// 200's weak tag and its `Vary: accept-encoding`.
    #[test]
    fn a_not_modified_answer_to_an_encoding_request_carries_the_encoded_validators() {
        for accept in ["gzip", "br, gzip;q=0.5", "*", "GZIP;Q=1"] {
            let revalidated = gzip_only().revalidated(&accepting(accept), response(StatusCode::NOT_MODIFIED, None, "\"x\""));
            pretty_assertions::assert_eq!(tag(&revalidated), Some(&HeaderValue::from_static("W/\"x\"")), "{accept}");
            pretty_assertions::assert_eq!(revalidated.headers().get(VARY), Some(&HeaderValue::from_static("accept-encoding")), "{accept}");
        }
    }

    /// A 304 answering a request that selects no coding the layer applies
    /// stands for the identity 200: it keeps the strong tag, and carries the
    /// `Vary: accept-encoding` the layer gives that 200 whatever the request
    /// asked for (RFC 7232 section 4.1).
    #[test]
    fn a_not_modified_answer_to_an_identity_request_keeps_the_strong_tag_and_varies_on_encoding() {
        let requests = ["identity", "br", "gzip;q=0", "gzip;q=0.000", "*;q=0"].map(accepting);
        for request in requests.iter().chain([&HeaderMap::new()]) {
            let revalidated = gzip_only().revalidated(request, response(StatusCode::NOT_MODIFIED, None, "\"x\""));
            pretty_assertions::assert_eq!(tag(&revalidated), Some(&HeaderValue::from_static("\"x\"")), "{request:?}");
            pretty_assertions::assert_eq!(
                revalidated.headers().get(VARY),
                Some(&HeaderValue::from_static("accept-encoding")),
                "{request:?}"
            );
        }
    }

    #[test]
    fn a_full_answer_the_layer_left_unencoded_gains_no_vary() {
        let unencoded = gzip_only().revalidated(&HeaderMap::new(), response(StatusCode::OK, None, "\"x\""));
        assert!(unencoded.headers().get(VARY).is_none());
    }

    #[test]
    fn an_existing_accept_encoding_vary_is_not_repeated() {
        let mut not_modified = response(StatusCode::NOT_MODIFIED, None, "\"x\"");
        not_modified
            .headers_mut()
            .insert(VARY, HeaderValue::from_static("NGSILD-Tenant, Accept-Encoding"));

        let revalidated = gzip_only().revalidated(&accepting("gzip"), not_modified);

        pretty_assertions::assert_eq!(revalidated.headers().get_all(VARY).iter().count(), 1);
    }
}
