//! The `Link` header of an answer read as the broker wrote it (IETF RFC 8288 section 3): the whole header as text, for a
//! test that requires it to hold or lack a text, and the target a relation names, unresolved, for a test whose subject
//! is whether that target carries an origin. A typed page resolves its targets into URLs instead.

use crate::wire::header_values;
use http::{HeaderMap, header::LINK};
use ngsi_ld::http::{
    ResponseHead,
    headers::{Link, RelationType},
};

/// Why the `Link` header of an answer does not read.
#[derive(Debug, thiserror::Error)]
#[error("the Link header does not read")]
pub struct LinkUnreadable;

/// Every `Link` field line of `headers`, joined by a comma.
#[must_use]
pub fn link_text(headers: &HeaderMap) -> String {
    header_values(headers, &LINK).join(", ")
}

/// The `Link` header of `headers`; `None` when they carry none.
///
/// # Errors
///
/// Returns [`LinkUnreadable`] when a `Link` header is sent and does not read.
pub fn link(headers: &HeaderMap) -> Result<Option<Link>, LinkUnreadable> {
    if !headers.contains_key(LINK) {
        return Ok(None);
    }
    // The head owns its header map; the caller keeps its own.
    ResponseHead::new(headers.clone()).typed::<Link>().into_valid().map(Some).ok_or(LinkUnreadable)
}

/// The target of the first link of `headers` whose relation is `relation`, as the broker wrote it; `None` when the
/// header names no such relation.
///
/// # Errors
///
/// Returns [`LinkUnreadable`] when a `Link` header is sent and does not read.
pub fn link_target(headers: &HeaderMap, relation: &RelationType) -> Result<Option<String>, LinkUnreadable> {
    Ok(link(headers)?.and_then(|link| link.find_by_rel(relation).next().map(|value| value.target().as_str().to_owned())))
}

#[cfg(test)]
mod tests {
    use crate::link_header::{link_target, link_text};
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderValue, header::LINK};
    use ngsi_ld::http::headers::RelationType;

    #[gtest]
    fn the_target_of_a_relation_is_read_as_written() -> Result<()> {
        let mut headers = HeaderMap::new();
        headers.append(
            LINK,
            HeaderValue::from_static(r#"<https://example.org/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#),
        );
        headers.append(LINK, HeaderValue::from_static(r#"</ngsi-ld/v1/entities?limit=1&cursor=a>; rel="next""#));
        expect_that!(link_target(&headers, &RelationType::Next)?, some(eq("/ngsi-ld/v1/entities?limit=1&cursor=a")));
        expect_that!(link_target(&headers, &RelationType::Prev)?, none());
        verify_that!(link_text(&headers), contains_substring("cursor="))
    }

    #[gtest]
    fn headers_without_a_link_name_no_target() -> Result<()> {
        let headers = HeaderMap::new();
        expect_that!(link_text(&headers), eq(""));
        verify_that!(link_target(&headers, &RelationType::Next)?, none())
    }
}
