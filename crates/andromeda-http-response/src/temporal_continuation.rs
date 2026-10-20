//! The `rel="next"` pointer of a partial temporal response (clause 6.3.10).
//!
//! Clause 6.3.10 gives a partial temporal response a `Content-Range` but no
//! request that resumes after it. The pointer is that request, spelled with
//! the standard temporal parameters only: the window and `lastN` of the
//! continuation replace the request's own, and every other parameter is kept,
//! so a client following it asks the same question of the rest of the
//! history. The `@context` and the `NGSILD-EntityMap` of the request travel
//! in headers, which the client sends again.

use crate::link_target::{append_typed_links, rebuilt_target};
use andromeda_http_headers::{link::relation_type::RelationType, media_type::ResponseMediaType};
use andromeda_ngsi_ld_lexicon::rfc3339::sub_second;
use andromeda_ngsi_ld_query::temporal::continuation::TemporalContinuation;
use axum::{http::Uri, response::Response};

/// The query parameters a continuation replaces (clause 6.3.10 and Table
/// 6.19.3.1-1).
const CONTINUATION_PARAMETERS: [&str; 4] = ["timerel", "timeAt", "endTimeAt", "lastN"];

/// Append the `rel="next"` pointer at the request `next` describes to
/// `response`, a partial answer to the request `uri` addressed, typed as the
/// response's `media_type` (clause 6.3.10).
///
/// Instants are written at the microsecond, as the `Content-Range` writes
/// them, so the continuation opens exactly where the page left off.
pub fn append_continuation_link(response: &mut Response, uri: &Uri, next: &TemporalContinuation, media_type: ResponseMediaType) {
    let (timerel, time_at, end_time_at) = next.interval.parts();
    let timerel = timerel.map(|relation| relation.to_string());
    let time_at = time_at.map(sub_second);
    let end_time_at = end_time_at.map(sub_second);
    let last_n = next.last_n.map(|last_n| last_n.to_string());
    let set: Vec<(&str, &str)> = CONTINUATION_PARAMETERS
        .into_iter()
        .zip([timerel.as_deref(), time_at.as_deref(), end_time_at.as_deref(), last_n.as_deref()])
        .filter_map(|(name, value)| value.map(|value| (name, value)))
        .collect();
    let target = rebuilt_target(uri, &CONTINUATION_PARAMETERS, &set);
    append_typed_links(response, vec![(RelationType::Next, target)], media_type);
}

#[cfg(test)]
mod tests {
    use crate::temporal_continuation::append_continuation_link;
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_query::temporal::{continuation::TemporalContinuation, window::TimeInterval};
    use axum::{
        http::{HeaderValue, StatusCode, Uri, header::LINK},
        response::{IntoResponse, Response},
    };
    use chrono::{DateTime, Utc};
    use std::num::NonZeroU32;
    use url::form_urlencoded;

    fn at(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
    }

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("a valid URI")
    }

    fn forward(interval: TimeInterval) -> TemporalContinuation {
        TemporalContinuation { interval, last_n: None }
    }

    /// The decoded query parameters of the `rel="next"` target `response`
    /// points at.
    fn next_parameters(response: &Response) -> Vec<(String, String)> {
        let header = response
            .headers()
            .get_all(LINK)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.contains(r#"rel="next""#))
            .expect("a next pointer");
        let target = header
            .split_once('<')
            .and_then(|(_, rest)| rest.split_once('>'))
            .map(|(target, _)| target)
            .expect("a link target");
        let query = target.split_once('?').map_or("", |(_, query)| query);
        form_urlencoded::parse(query.as_bytes()).into_owned().collect()
    }

    fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
    }

    fn continued(request: &str, next: &TemporalContinuation, media_type: ResponseMediaType) -> Response {
        let mut response = StatusCode::PARTIAL_CONTENT.into_response();
        append_continuation_link(&mut response, &uri(request), next, media_type);
        response
    }

    /// The window is replaced at the microsecond and `attrs` stays.
    #[test]
    fn an_after_continuation_replaces_the_window_and_keeps_the_other_parameters() {
        let response = continued(
            "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Vehicle:1?timerel=after&timeAt=2020-01-01T00:00:00Z&attrs=speed",
            &forward(TimeInterval::After(at("2020-01-01T01:19:00.000001Z"))),
            ResponseMediaType::Json,
        );
        pretty_assertions::assert_eq!(
            next_parameters(&response),
            owned(&[("attrs", "speed"), ("timerel", "after"), ("timeAt", "2020-01-01T01:19:00.000001Z")])
        );
    }

    /// What remains of a `before` window is a `between` window closing where
    /// the request's closed.
    #[test]
    fn a_before_request_continues_as_a_between_window() {
        let response = continued(
            "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Vehicle:1?timerel=before&timeAt=2020-01-01T02:00:00Z",
            &forward(TimeInterval::Between {
                from: at("2020-01-01T01:19:00.000001Z"),
                to: at("2020-01-01T02:00:00Z"),
            }),
            ResponseMediaType::Json,
        );
        pretty_assertions::assert_eq!(
            next_parameters(&response),
            owned(&[
                ("timerel", "between"),
                ("timeAt", "2020-01-01T01:19:00.000001Z"),
                ("endTimeAt", "2020-01-01T02:00:00Z")
            ])
        );
    }

    /// A continuation with no closing instant carries no `endTimeAt`.
    #[test]
    fn an_after_continuation_of_a_between_request_drops_its_end_time_at() {
        let response = continued(
            "/t?timerel=between&timeAt=2020-01-01T00:00:00Z&endTimeAt=2020-01-01T02:00:00Z",
            &forward(TimeInterval::After(at("2020-01-01T01:19:00.000001Z"))),
            ResponseMediaType::Json,
        );
        assert!(next_parameters(&response).iter().all(|(key, _)| key != "endTimeAt"));
    }

    #[test]
    fn a_backward_continuation_replaces_the_last_n() {
        let response = continued(
            "/t?lastN=5000&timerel=before&timeAt=2020-01-01T02:00:00Z",
            &TemporalContinuation {
                interval: TimeInterval::Before(at("2020-01-01T01:01:00Z")),
                last_n: NonZeroU32::new(4000),
            },
            ResponseMediaType::Json,
        );
        pretty_assertions::assert_eq!(
            next_parameters(&response),
            owned(&[("timerel", "before"), ("timeAt", "2020-01-01T01:01:00Z"), ("lastN", "4000")])
        );
    }

    /// Clause 6.3.10: the pointer's `type` is the response's media type.
    #[test]
    fn the_pointer_is_typed_as_the_response() {
        for (media_type, expected) in [
            (ResponseMediaType::Json, "application/json"),
            (ResponseMediaType::JsonLd, "application/ld+json"),
        ] {
            let response = continued(
                "/t?timerel=after&timeAt=2020-01-01T00:00:00Z",
                &forward(TimeInterval::After(at("2020-01-02T00:00:00Z"))),
                media_type,
            );
            let header = response
                .headers()
                .get(LINK)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            assert!(header.contains(r#"rel="next""#), "{header}");
            assert!(header.contains(&format!(r#"type="{expected}""#)), "{header}");
        }
    }

    /// A response carrying its `@context` in a `Link` header keeps it.
    #[test]
    fn a_context_link_already_on_the_response_is_kept() {
        let context = r#"<https://example.com/context.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;
        let mut response = StatusCode::PARTIAL_CONTENT.into_response();
        response.headers_mut().insert(LINK, HeaderValue::from_static(context));
        append_continuation_link(
            &mut response,
            &uri("/t?timerel=after&timeAt=2020-01-01T00:00:00Z"),
            &forward(TimeInterval::After(at("2020-01-02T00:00:00Z"))),
            ResponseMediaType::Json,
        );
        let values: Vec<&str> = response.headers().get_all(LINK).iter().filter_map(|value| value.to_str().ok()).collect();
        pretty_assertions::assert_eq!(values.len(), 2, "{values:?}");
        pretty_assertions::assert_eq!(values[0], context);
        assert!(values[1].contains(r#"rel="next""#), "{values:?}");
    }
}
