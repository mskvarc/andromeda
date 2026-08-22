//! The query string a map-creating request is forwarded with (clauses
//! 5.14.4.4 and 5.14.5.4): the whole query when entities are not split, the
//! entity selectors alone when they are, because a filter cannot be decided on
//! a partial entity. A temporal forward keeps the temporal window either way,
//! as the window is the query itself rather than a filter on it.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
use axum::http::Uri as HttpUri;
use url::form_urlencoded;

/// Whether the filters travel with the forwarded request or stay behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForwardedFilters {
    /// Entities are complete at every source: the filters are forwarded.
    Forwarded,
    /// Entities may be split: only the selectors are forwarded.
    Stripped,
}

impl ForwardedFilters {
    /// Resolve from whether the request's entities may be split.
    pub(crate) const fn for_split(split_entities: bool) -> ForwardedFilters {
        if split_entities {
            ForwardedFilters::Stripped
        } else {
            ForwardedFilters::Forwarded
        }
    }
}

/// The parameters that survive stripping on an entity query.
const ENTITY_SELECTOR_PARAMETERS: [&str; 4] = ["id", "type", "idPattern", "entityMapLifetime"];

/// The parameters that survive stripping on a temporal query.
const TEMPORAL_SELECTOR_PARAMETERS: [&str; 9] = [
    "id",
    "type",
    "idPattern",
    "entityMapLifetime",
    "timerel",
    "timeAt",
    "endTimeAt",
    "timeproperty",
    "lastN",
];

/// Keep the pairs of `original` whose key is one of `kept`.
fn keep_parameters(original: &str, kept: &[&str]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in form_urlencoded::parse(original.as_bytes()) {
        if kept.contains(&key.as_ref()) {
            serializer.append_pair(&key, &value);
        }
    }
    serializer.finish()
}

/// The forwarded form of a `GET /entityMaps` query string.
pub(crate) fn forwarded_query_string(uri: &HttpUri, filters: ForwardedFilters) -> String {
    let original = uri.query().unwrap_or("");
    match filters {
        ForwardedFilters::Forwarded => original.to_string(),
        ForwardedFilters::Stripped => keep_parameters(original, &ENTITY_SELECTOR_PARAMETERS),
    }
}

/// The forwarded form of a `GET /temporal/entityMaps` query string.
pub(crate) fn forwarded_temporal_query_string(uri: &HttpUri, filters: ForwardedFilters) -> String {
    let original = uri.query().unwrap_or("");
    match filters {
        ForwardedFilters::Forwarded => original.to_string(),
        ForwardedFilters::Stripped => keep_parameters(original, &TEMPORAL_SELECTOR_PARAMETERS),
    }
}

/// Append the entity selectors of a query body as GET parameters.
fn append_selectors(serializer: &mut form_urlencoded::Serializer<'_, String>, id: Option<&[Uri]>, entity_type: Option<String>, id_pattern: Option<&str>) {
    if let Some(ids) = id {
        serializer.append_pair("id", &ids.iter().map(Uri::as_str).collect::<Vec<_>>().join(","));
    }
    if let Some(entity_type) = entity_type {
        serializer.append_pair("type", &entity_type);
    }
    if let Some(pattern) = id_pattern {
        serializer.append_pair("idPattern", pattern);
    }
}

/// The query string a `POST /entityMaps` body is forwarded as: the body's
/// entity selectors rendered as GET parameters, so a peer answers the same
/// request over either binding, with the request's own URL parameters and
/// `attrs` beside them when the filters travel.
pub(crate) fn body_query_string(query: &EntityQueryParameters, filters: ForwardedFilters, uri: &HttpUri) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    append_selectors(
        &mut serializer,
        query.id.as_deref(),
        query.entity_type.as_ref().map(ToString::to_string),
        query.id_pattern.as_ref().map(|pattern| pattern.as_str()),
    );
    if filters == ForwardedFilters::Forwarded {
        if let Some(attrs) = &query.attrs {
            serializer.append_pair("attrs", &attrs.iter().map(|attr| attr.as_str()).collect::<Vec<_>>().join(","));
        }
        for (key, value) in form_urlencoded::parse(uri.query().unwrap_or("").as_bytes()) {
            serializer.append_pair(&key, &value);
        }
    }
    serializer.finish()
}

/// The query string a `POST /temporal/entityMaps` body is forwarded as.
pub(crate) fn temporal_body_query_string(params: &TemporalQueryParameters, filters: ForwardedFilters) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    append_selectors(
        &mut serializer,
        params.id.as_deref(),
        params.entity_type.as_ref().map(ToString::to_string),
        params.id_pattern.as_ref().map(|pattern| pattern.as_str()),
    );
    if let Some(timerel) = params.timerel {
        serializer.append_pair("timerel", &timerel.to_string());
    }
    if let Some(time_at) = params.time_at {
        serializer.append_pair("timeAt", &time_at.to_rfc3339());
    }
    if let Some(end_time_at) = params.end_time_at {
        serializer.append_pair("endTimeAt", &end_time_at.to_rfc3339());
    }
    if let Some(timeproperty) = params.timeproperty {
        serializer.append_pair("timeproperty", &timeproperty.to_string());
    }
    if let Some(last_n) = params.last_n {
        serializer.append_pair("lastN", &last_n.to_string());
    }
    if filters == ForwardedFilters::Forwarded
        && let Some(attrs) = &params.attrs
    {
        serializer.append_pair("attrs", &attrs.iter().map(|attr| attr.as_str()).collect::<Vec<_>>().join(","));
    }
    serializer.finish()
}

#[cfg(test)]
mod tests {
    use crate::forwarded_query_string::{ForwardedFilters, forwarded_query_string, forwarded_temporal_query_string};
    use axum::http::Uri;

    #[test]
    fn a_split_forward_keeps_the_entity_selectors_and_drops_the_filters() {
        let uri: Uri = "/ngsi-ld/v1/entityMaps?type=Vehicle&q=speed%3E10&attrs=speed&id=urn:a&scopeQ=/Madrid&entityMapLifetime=PT1H"
            .parse()
            .expect("valid URI");
        pretty_assertions::assert_eq!(
            forwarded_query_string(&uri, ForwardedFilters::Stripped),
            "type=Vehicle&id=urn%3Aa&entityMapLifetime=PT1H"
        );
        pretty_assertions::assert_eq!(forwarded_query_string(&uri, ForwardedFilters::Forwarded), uri.query().expect("query"));
    }

    #[test]
    fn a_split_temporal_forward_keeps_the_window_beside_the_selectors() {
        let uri: Uri = "/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&q=speed%3E10&timerel=after&timeAt=2020-01-01T00:00:00Z&lastN=3"
            .parse()
            .expect("valid URI");
        pretty_assertions::assert_eq!(
            forwarded_temporal_query_string(&uri, ForwardedFilters::Stripped),
            "type=Vehicle&timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&lastN=3"
        );
    }

    #[test]
    fn the_filter_policy_follows_the_split_resolution() {
        pretty_assertions::assert_eq!(ForwardedFilters::for_split(true), ForwardedFilters::Stripped);
        pretty_assertions::assert_eq!(ForwardedFilters::for_split(false), ForwardedFilters::Forwarded);
    }
}
