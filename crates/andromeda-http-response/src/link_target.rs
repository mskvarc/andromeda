//! The targets of the `Link` pointers a response carries (clause 6.3.10,
//! RFC 8288).
//!
//! A pointer names the request the response answered with some of its query
//! parameters replaced, so a followed pointer asks the same question of
//! another part of the answer. Its path is the request's path below the
//! configured `apiRoot`'s path, so a client behind a proxy that serves the
//! broker under a prefix follows it through that prefix (clause 6.2). Every
//! pointer is typed with the negotiated response media type, so the client
//! follows it in the representation it asked for.

use crate::api_root_path::api_root_path;
use andromeda_http_headers::{
    link::{header::Link, relation_type::RelationType, value::LinkValue},
    media_type::ResponseMediaType,
};
use axum::{
    http::{Uri, header::LINK},
    response::Response,
};
use iri_rs::IriRefBuf;
use mediatype::MediaTypeBuf;
use thiserror::Error;
use url::{Url, form_urlencoded};

/// The request a response's pointers rebuild, as a client reaches it through
/// the `apiRoot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointedRequest {
    /// The configured `apiRoot`, whose path every pointer starts with; `None`
    /// when none is configured and the request's own path is the one a client
    /// reaches.
    api_root: Option<Url>,
    /// The URI the request reached the broker with, mount prefix included.
    addressed: Uri,
}

impl PointedRequest {
    /// The request that reached the broker with `addressed`, reached by a
    /// client through `api_root`.
    #[must_use]
    pub const fn new(api_root: Option<Url>, addressed: Uri) -> PointedRequest {
        PointedRequest { api_root, addressed }
    }

    /// The query string the request carried.
    #[must_use]
    pub fn query(&self) -> Option<&str> {
        self.addressed.query()
    }

    /// The path a client reaches the request's resource on.
    fn path(&self) -> String {
        format!("{}{}", api_root_path(self.api_root.as_ref()), self.addressed.path())
    }

    /// The request a pointer of a query sent by POST is followed as: the GET
    /// binding of the same query below the same API root, with the URL
    /// parameters the POST carried.
    ///
    /// Clauses 6.23.3.1 and 6.24.3.1 answer the POST binding on
    /// `entityOperations/query` beside the `entities` resource whose GET
    /// binding answers the same query (clauses 6.4.3.2 and 6.18.3.2).
    ///
    /// # Errors
    ///
    /// [`NotAPostedQuery`] for a request addressed to any other path, which no
    /// POST query binding is routed on.
    pub fn into_query_binding(self) -> Result<PointedRequest, NotAPostedQuery> {
        let PointedRequest { api_root, addressed } = self;
        let root = addressed.path().strip_suffix(POSTED_QUERY_PATH).ok_or(NotAPostedQuery)?;
        let target = match addressed.query() {
            Some(query) => format!("{root}{QUERY_PATH}?{query}"),
            None => format!("{root}{QUERY_PATH}"),
        };
        Ok(PointedRequest {
            api_root,
            addressed: Uri::try_from(target).map_err(|_| NotAPostedQuery)?,
        })
    }
}

/// The path a query sent by POST is answered on, below the API root of its
/// operation family.
const POSTED_QUERY_PATH: &str = "/entityOperations/query";

/// The path the GET binding of the same query is answered on, below the same
/// API root.
const QUERY_PATH: &str = "/entities";

/// A request whose pointers were to be followed as a GET query, addressed to
/// no POST query binding.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("the request was not addressed to a query binding answered by POST")]
pub struct NotAPostedQuery;

/// The target `request` rebuilt with every parameter named in `replaced`
/// removed from its query string and `set` appended in order; every other
/// parameter keeps its place and value.
pub(crate) fn rebuilt_target(request: &PointedRequest, replaced: &[&str], set: &[(&str, &str)]) -> String {
    let kept = request.query().map_or_default(|query| {
        form_urlencoded::parse(query.as_bytes())
            .filter(|(key, _)| !replaced.contains(&key.as_ref()))
            .collect::<Vec<_>>()
    });
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(kept)
        .extend_pairs(set.iter().copied())
        .finish();
    let path = request.path();
    if query.is_empty() { path } else { format!("{path}?{query}") }
}

/// Append one `Link` header holding every `(relation, target)` of `links` to
/// `response`, each typed as `media_type` (clause 6.3.10).
///
/// The header is appended rather than inserted: a response rendered with the
/// `@context` in the headers (clause 6.3.5) already carries a `Link` header,
/// and replacing it would drop the context the client needs.
pub(crate) fn append_typed_links(response: &mut Response, links: Vec<(RelationType, String)>, media_type: ResponseMediaType) {
    let media_type = MediaTypeBuf::from(&media_type.media_type());
    let values = links
        .into_iter()
        .filter_map(|(relation, target)| {
            // Each link value needs its own owned media type, since the closure runs once per link.
            IriRefBuf::new(target)
                .ok()
                .map(|target| LinkValue::with_params(target, Some(relation), Some(media_type.clone())))
        })
        .collect::<Vec<_>>();
    if let Some(value) = Link::from_values(values).and_then(|link| link.to_header_value()) {
        response.headers_mut().append(LINK, value);
    }
}

#[cfg(test)]
mod tests {
    use crate::link_target::{NotAPostedQuery, PointedRequest, rebuilt_target};
    use pretty_assertions::assert_eq;
    use url::{Url, form_urlencoded};

    fn request(api_root: Option<&str>, raw: &str) -> PointedRequest {
        PointedRequest::new(api_root.map(|root| Url::parse(root).expect("a valid URL")), raw.parse().expect("a valid URI"))
    }

    fn pairs(target: &str) -> Vec<(String, String)> {
        let query = target.split_once('?').map_or("", |(_, query)| query);
        form_urlencoded::parse(query.as_bytes()).into_owned().collect()
    }

    fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
    }

    /// The replaced parameters leave their places, the kept ones stay in
    /// order, and the set ones follow them, percent-encoded.
    #[test]
    fn a_rebuilt_target_replaces_the_named_parameters_and_keeps_the_others_in_order() {
        let pointed = request(
            None,
            "/ngsi-ld/v1/temporal/entities/urn:e:1?attrs=speed&timeAt=2020-01-01T00%3A00%3A00Z&options=temporalValues&lastN=5",
        );
        let target = rebuilt_target(&pointed, &["timeAt", "lastN"], &[("timeAt", "2020-01-01T01:19:00Z"), ("lastN", "4")]);
        assert!(target.starts_with("/ngsi-ld/v1/temporal/entities/urn:e:1?"), "{target}");
        assert!(target.contains("timeAt=2020-01-01T01%3A19%3A00Z"), "{target}");
        pretty_assertions::assert_eq!(
            pairs(&target),
            owned(&[
                ("attrs", "speed"),
                ("options", "temporalValues"),
                ("timeAt", "2020-01-01T01:19:00Z"),
                ("lastN", "4")
            ])
        );
    }

    #[test]
    fn a_rebuilt_target_without_any_parameter_is_the_bare_path() {
        let pointed = request(None, "/ngsi-ld/v1/entities?limit=10");
        pretty_assertions::assert_eq!(rebuilt_target(&pointed, &["limit"], &[]), "/ngsi-ld/v1/entities");
    }

    #[test]
    fn a_rebuilt_target_starts_with_the_path_of_the_configured_api_root() {
        let pointed = request(Some("https://proxy.example/context-broker"), "/ngsi-ld/v1/entities?type=V&limit=2");
        pretty_assertions::assert_eq!(
            rebuilt_target(&pointed, &["limit"], &[("limit", "3")]),
            "/context-broker/ngsi-ld/v1/entities?type=V&limit=3"
        );
    }

    /// The entity and the temporal POST query bindings are each followed on
    /// the `entities` resource below their own API root, keeping the URL
    /// parameters and the configured `apiRoot` the POST was reached through.
    #[test]
    fn a_posted_query_is_followed_on_the_get_binding_below_the_same_api_root() {
        for (posted, followed) in [
            ("/ngsi-ld/v1/entityOperations/query?limit=2", "/context-broker/ngsi-ld/v1/entities?limit=3"),
            (
                "/ngsi-ld/v1/temporal/entityOperations/query?limit=2",
                "/context-broker/ngsi-ld/v1/temporal/entities?limit=3",
            ),
        ] {
            let pointed = request(Some("https://proxy.example/context-broker"), posted)
                .into_query_binding()
                .expect("a POST query binding");
            pretty_assertions::assert_eq!(rebuilt_target(&pointed, &["limit"], &[("limit", "3")]), followed);
        }
        let bare = request(None, "/ngsi-ld/v1/entityOperations/query")
            .into_query_binding()
            .expect("a POST query binding");
        pretty_assertions::assert_eq!(rebuilt_target(&bare, &[], &[]), "/ngsi-ld/v1/entities");
    }

    #[test]
    fn a_request_to_any_other_path_is_no_posted_query() {
        assert_eq!(request(None, "/ngsi-ld/v1/entities?type=V").into_query_binding(), Err(NotAPostedQuery));
    }

    #[test]
    fn a_rebuilt_target_under_an_api_root_without_a_path_is_the_request_path() {
        let pointed = request(Some("https://proxy.example"), "/ngsi-ld/v1/entities?type=V&limit=2");
        pretty_assertions::assert_eq!(rebuilt_target(&pointed, &["limit"], &[("limit", "3")]), "/ngsi-ld/v1/entities?type=V&limit=3");
    }
}
