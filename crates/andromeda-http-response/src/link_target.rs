//! The targets of the `Link` pointers a response carries (clause 6.3.10,
//! RFC 8288).
//!
//! A pointer names the request the response answered with some of its query
//! parameters replaced, so a followed pointer asks the same question of
//! another part of the answer. Every pointer is typed with the negotiated
//! response media type, so the client follows it in the representation it
//! asked for.

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
use url::form_urlencoded;

/// The target `uri` rebuilt with every parameter named in `replaced` removed
/// from its query string and `set` appended in order; every other parameter
/// keeps its place and value.
pub(crate) fn rebuilt_target(uri: &Uri, replaced: &[&str], set: &[(&str, &str)]) -> String {
    let kept = uri
        .query()
        .map(|query| {
            form_urlencoded::parse(query.as_bytes())
                .filter(|(key, _)| !replaced.contains(&key.as_ref()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(kept)
        .extend_pairs(set.iter().copied())
        .finish();
    if query.is_empty() {
        uri.path().to_owned()
    } else {
        format!("{}?{}", uri.path(), query)
    }
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
            // clone: each link value owns its copy of the negotiated media type.
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
    use crate::link_target::rebuilt_target;
    use axum::http::Uri;
    use url::form_urlencoded;

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
        let uri: Uri = "/ngsi-ld/v1/temporal/entities/urn:e:1?attrs=speed&timeAt=2020-01-01T00%3A00%3A00Z&options=temporalValues&lastN=5"
            .parse()
            .expect("a valid URI");
        let target = rebuilt_target(&uri, &["timeAt", "lastN"], &[("timeAt", "2020-01-01T01:19:00Z"), ("lastN", "4")]);
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
        let uri: Uri = "/ngsi-ld/v1/entities?limit=10".parse().expect("a valid URI");
        pretty_assertions::assert_eq!(rebuilt_target(&uri, &["limit"], &[]), "/ngsi-ld/v1/entities");
    }
}
