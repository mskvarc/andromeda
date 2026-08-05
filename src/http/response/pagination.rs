//! Shared pagination link construction for NGSI-LD collection responses.
//!
//! Provides common logic for building pagination `Link` headers across
//! subscription, CSR subscription, and CSR query endpoints.

use andromeda_ngsi_ld_http_headers::link::{header::Link, value::LinkValue};
use axum::{http::Uri, response::Response};
use url::form_urlencoded;

/// Pagination link style controlling which link relations are included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationStyle {
    /// Only `prev`/`next` links using `limit` and `offset` query parameters.
    OffsetPrevNext,
    /// Only `prev`/`next` links using `limit` and `page` query parameters.
    PagePrevNext,
    /// `first`/`last`/`prev`/`next` links using `limit` and `offset` query parameters.
    OffsetFull,
}

/// Whether to include a JSON-LD media type on pagination link values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationMediaType {
    /// Include `type="application/ld+json"` on link values.
    JsonLd,
    /// No media type on link values.
    None,
}

/// Build a pagination link URL by replacing pagination parameters in the query string.
fn build_page_link(uri: &Uri, style: PaginationStyle, param1: (&str, i64), param2: (&str, i64)) -> String {
    let strip_params: &[&str] = match style {
        PaginationStyle::OffsetPrevNext | PaginationStyle::OffsetFull => &["limit", "offset"],
        PaginationStyle::PagePrevNext => &["limit", "offset", "page"],
    };

    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(query) = uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if strip_params.contains(&k.as_ref()) {
                continue;
            }
            pairs.push((k.into_owned(), v.into_owned()));
        }
    }
    pairs.push((param1.0.to_string(), param1.1.to_string()));
    pairs.push((param2.0.to_string(), param2.1.to_string()));

    let query = form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();

    if query.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), query)
    }
}

/// Append pagination `Link` headers to a response.
///
/// Supports three pagination styles:
/// - [`PaginationStyle::OffsetPrevNext`]: `prev`/`next` using offset-based parameters.
/// - [`PaginationStyle::PagePrevNext`]: `prev`/`next` using page-based parameters.
/// - [`PaginationStyle::OffsetFull`]: `first`/`last`/`prev`/`next` using offset-based parameters.
pub fn append_pagination_links(
    response: &mut Response,
    uri: &Uri,
    limit: i64,
    offset: i64,
    total: i64,
    style: PaginationStyle,
    media_type: PaginationMediaType,
) {
    if limit <= 0 || total <= 0 {
        return;
    }

    let mt = match media_type {
        PaginationMediaType::JsonLd => Some(LinkValue::JSON_LD_MEDIA_TYPE),
        PaginationMediaType::None => None,
    };

    let mut links = Vec::new();

    match style {
        PaginationStyle::OffsetPrevNext => {
            if offset > 0 {
                let prev_offset = (offset - limit).max(0);
                links.push(("prev", build_page_link(uri, style, ("limit", limit), ("offset", prev_offset))));
            }
            if offset + limit < total {
                links.push(("next", build_page_link(uri, style, ("limit", limit), ("offset", offset + limit))));
            }
        }
        PaginationStyle::PagePrevNext => {
            let current_page = (offset / limit) + 1;
            let total_pages = (total + limit - 1) / limit;
            if current_page > 1 {
                links.push(("prev", build_page_link(uri, style, ("limit", limit), ("page", current_page - 1))));
            }
            if current_page < total_pages {
                links.push(("next", build_page_link(uri, style, ("limit", limit), ("page", current_page + 1))));
            }
        }
        PaginationStyle::OffsetFull => {
            let last_offset = ((total - 1) / limit) * limit;
            links.push(("first", build_page_link(uri, style, ("limit", limit), ("offset", 0))));
            links.push(("last", build_page_link(uri, style, ("limit", limit), ("offset", last_offset))));
            if offset > 0 {
                let prev_offset = (offset - limit).max(0);
                links.push(("prev", build_page_link(uri, style, ("limit", limit), ("offset", prev_offset))));
            }
            if offset + limit < total {
                links.push(("next", build_page_link(uri, style, ("limit", limit), ("offset", offset + limit))));
            }
        }
    }

    let values = links
        .into_iter()
        .map(|(rel, target)| LinkValue::with_params(&target, Some(rel), mt))
        .collect::<Vec<_>>();
    if let Some(link) = Link::from_values(values)
        && let Some(value) = link.to_header_value()
    {
        response.headers_mut().append("Link", value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, response::IntoResponse};

    fn empty_response() -> Response {
        StatusCode::OK.into_response()
    }

    fn parse_uri(s: &str) -> Uri {
        s.parse().expect("valid URI")
    }

    #[test]
    fn offset_prev_next_adds_next_when_more_pages() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions?limit=10&offset=0");
        append_pagination_links(&mut response, &uri, 10, 0, 25, PaginationStyle::OffsetPrevNext, PaginationMediaType::JsonLd);
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("rel=\"next\""));
        assert!(!link_str.contains("rel=\"prev\""));
    }

    #[test]
    fn offset_prev_next_adds_prev_when_not_first_page() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions?limit=10&offset=10");
        append_pagination_links(&mut response, &uri, 10, 10, 25, PaginationStyle::OffsetPrevNext, PaginationMediaType::JsonLd);
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    #[test]
    fn offset_prev_next_no_links_when_single_page() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions");
        append_pagination_links(&mut response, &uri, 10, 0, 5, PaginationStyle::OffsetPrevNext, PaginationMediaType::JsonLd);
        assert!(response.headers().get("Link").is_none());
    }

    #[test]
    fn page_prev_next_uses_page_param() {
        let mut response = empty_response();
        let uri = parse_uri("/csourceSubscriptions?limit=10&offset=10");
        append_pagination_links(&mut response, &uri, 10, 10, 50, PaginationStyle::PagePrevNext, PaginationMediaType::JsonLd);
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("page="));
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    #[test]
    fn offset_full_includes_first_last() {
        let mut response = empty_response();
        let uri = parse_uri("/csourceRegistrations?limit=10&offset=10");
        append_pagination_links(&mut response, &uri, 10, 10, 50, PaginationStyle::OffsetFull, PaginationMediaType::None);
        let link = response.headers().get("Link").expect("Link header");
        let link_str = link.to_str().expect("valid string");
        assert!(link_str.contains("rel=\"first\""));
        assert!(link_str.contains("rel=\"last\""));
        assert!(link_str.contains("rel=\"prev\""));
        assert!(link_str.contains("rel=\"next\""));
    }

    #[test]
    fn no_links_when_total_is_zero() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions");
        append_pagination_links(&mut response, &uri, 10, 0, 0, PaginationStyle::OffsetPrevNext, PaginationMediaType::JsonLd);
        assert!(response.headers().get("Link").is_none());
    }

    #[test]
    fn no_links_when_limit_is_zero() {
        let mut response = empty_response();
        let uri = parse_uri("/subscriptions");
        append_pagination_links(&mut response, &uri, 0, 0, 10, PaginationStyle::OffsetPrevNext, PaginationMediaType::JsonLd);
        assert!(response.headers().get("Link").is_none());
    }
}
