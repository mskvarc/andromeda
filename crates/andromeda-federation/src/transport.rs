//! HTTP execution and header construction for federated requests.

use crate::{
    FederationService,
    cache::{CacheKey, Cacheability, ForwardedGetCache},
    error::FederationError,
    forwarded_payload::{ForwardedPayload, build_forwarded_payload},
    forwarding_url::with_local_scope,
    outcome::{ForwardGetOutcome, TransportFailure},
    registration_loops,
    request::TriggeringRequest,
};
use andromeda_http_headers::{
    accept::Accept,
    link::header::Link,
    media_type::ResponseMediaType,
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_tenant::NgsiLdTenant,
    ngsild_warning::NgsiLdWarning,
    via::Via,
    warning::Warning,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, registration::ContextSourceRegistration, registration_management::LocalScope};
use headers::{Header, HeaderMapExt};
use http::{HeaderMap, HeaderName, HeaderValue};
use iri_rs::{Iri, iri};
use std::{iter, str::FromStr};
use tracing::{debug, warn};
use url::Url;

/// The value of a `contextSourceInfo` pair that is copied from the triggering request (clause 6.3.19).
const FROM_TRIGGERING_REQUEST: Iri<&'static str> = iri!("urn:ngsi-ld:request");

/// One forwarded request, fully decided: where it goes and what it carries.
pub(crate) struct ForwardedRequest<'a> {
    /// The HTTP method.
    pub(crate) method: reqwest::Method,
    /// The forwarded URL, before the registration's local scope is applied.
    pub(crate) url: Url,
    /// The representation and context of the body.
    pub(crate) payload: &'a ForwardedPayload,
    /// The registration being contacted.
    pub(crate) registration: &'a ContextSourceRegistration,
    /// The request that triggered the forwarding.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The EntityMap the registration's context source created for this
    /// query, sent back to it in `NGSILD-EntityMap` (clause 5.5.14).
    pub(crate) linked_map: Option<&'a Uri>,
}

impl FederationService {
    /// Read a registration's `contextSourceInfo`; stored registrations passed
    /// the clause 4.3.6.6 restrictions at registration time, so a failure here
    /// is an invariant violation rather than a request error.
    pub(crate) fn context_source_info<'a>(&self, registration: &'a ContextSourceRegistration) -> Result<ContextSourceInfo<'a>, FederationError> {
        let pairs = registration.access.context_source_info.as_deref().unwrap_or_default();
        ContextSourceInfo::try_from(pairs)
            .map_err(|error| FederationError::internal(format!("registration {} carries an invalid contextSourceInfo: {error}", registration.id)))
    }

    /// Decide the representation one forwarded request carries for `registration`.
    pub(crate) async fn payload_for(
        &self,
        registration: &ContextSourceRegistration,
        inbound: &ContextSpec,
        body: Option<&[u8]>,
    ) -> Result<ForwardedPayload, FederationError> {
        let info = self.context_source_info(registration)?;
        build_forwarded_payload(&self.engine, inbound, body, &info, registration.access.jsonld_context.as_ref()).await
    }

    /// Make a GET request to a context source endpoint.
    pub(crate) async fn forward_get(
        &self,
        registration: &ContextSourceRegistration,
        url: Result<Url, FederationError>,
        inbound: &ContextSpec,
        triggering: TriggeringRequest<'_>,
        cacheability: Cacheability,
        linked_map: Option<&Uri>,
    ) -> ForwardGetOutcome {
        if registration_loops(registration, triggering.via) {
            return ForwardGetOutcome::LoopDetected;
        }
        let url = match url {
            Ok(url) => url,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::Unreachable { detail: error.to_string() }),
        };
        let payload = match self.payload_for(registration, inbound, None).await {
            Ok(payload) => payload,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::Unreachable { detail: error.to_string() }),
        };
        let cache_duration = match cacheability {
            Cacheability::Cacheable => ForwardedGetCache::duration_of(registration),
            Cacheability::Uncacheable => None,
        };
        let cache_key = cache_duration.map(|_| CacheKey::new(registration, &url, &payload.link_contexts));
        if let Some(key) = &cache_key
            && let Some((body, warnings)) = self.cache.lookup(key)
        {
            return ForwardGetOutcome::Success(body, warnings);
        }
        if let Some(remaining) = self.cooldowns.remaining(registration) {
            // Clause 5.2.34: a request inside the cooldown window is answered
            // with a timeout for the registration, without contacting it.
            return ForwardGetOutcome::Transport(TransportFailure::Timeout {
                detail: format!("registration is cooling down for another {}ms after a failure", remaining.as_millis()),
            });
        }
        debug!("CS forwarding GET {}", url);

        let request = match self.build_request(ForwardedRequest {
            method: reqwest::Method::GET,
            url,
            payload: &payload,
            registration,
            triggering,
            linked_map,
        }) {
            Ok(request) => request,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::Unreachable { detail: error.to_string() }),
        };

        let sent = match self.send_request(registration, request).await {
            Ok(sent) => sent,
            Err(failure) => return ForwardGetOutcome::Transport(failure),
        };

        if sent.status == reqwest::StatusCode::NOT_FOUND {
            return ForwardGetOutcome::NotFound;
        }
        if !sent.status.is_success() {
            return ForwardGetOutcome::ErrorStatus(sent.status.as_u16());
        }

        // Propagate IANA Warning 110 (Response is Stale) from the context source (clause 6.3.17).
        let mut warnings = Vec::new();
        if let Some(Warning { code: 110, text, .. }) = sent.warning {
            warnings.push(NgsiLdWarning::ResponseIsStale(text));
        }

        let text = String::from_utf8_lossy(&sent.body);
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => {
                if let (Some(key), Some(duration)) = (cache_key, cache_duration) {
                    self.cache.store(key, &value, &warnings, duration);
                }
                ForwardGetOutcome::Success(value, warnings)
            }
            Err(e) => {
                warn!("Failed to parse CS response as JSON: {}. Body: {}", e, &text[..text.len().min(200)]);
                ForwardGetOutcome::InvalidPayload(format!("Failed to parse context source response as JSON: {}", e))
            }
        }
    }

    /// Build a request with every header clause 6.3.19 and clause 6.3.18 call
    /// for: the context `Link`s, `Accept`, the body with its `Content-Type`,
    /// the `contextSourceInfo` custom headers, `NGSILD-Tenant` and `Via`.
    pub(crate) fn build_request(&self, forwarded: ForwardedRequest<'_>) -> Result<reqwest::RequestBuilder, FederationError> {
        let ForwardedRequest {
            method,
            url,
            payload,
            registration,
            triggering,
            linked_map,
        } = forwarded;
        let access = &registration.access;
        let info = self.context_source_info(registration)?;

        // Clause 5.5.13 through clause 6.3.18: a `localOnly` registration is
        // asked for its own data only.
        let url = match access.management.as_ref().and_then(|management| management.local_only) {
            Some(LocalScope::LocalOnly) => with_local_scope(url),
            Some(LocalScope::Cascading) | None => url,
        };

        let mut typed_headers = HeaderMap::new();
        typed_headers.typed_insert(headers::Connection::close());
        for context in &payload.link_contexts {
            let link = Link::jsonld_context(context.clone()); // clone: the header owns the IRI reference.
            if let Some(value) = link.to_header_value() {
                typed_headers.append(Link::name().clone(), value);
            }
        }

        let accept = info
            .accept
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| access.accept.as_ref().map(ToString::to_string))
            .and_then(|raw| {
                let value = HeaderValue::from_str(&raw).ok()?;
                let mut values = iter::once(&value);
                Accept::decode(&mut values).ok()
            })
            .unwrap_or_else(|| Accept::single(ResponseMediaType::Json));
        typed_headers.typed_insert(accept);
        if payload.body.is_some() {
            typed_headers.typed_insert(payload.media_type);
        }

        let mut request = self.http_client.request(method, url).headers(typed_headers);
        request = apply_custom_headers(request, &info, triggering.headers)?;
        if let Some(body) = &payload.body {
            request = request.body(body.clone()); // clone: reqwest owns the body it sends
        }

        if let Some(name) = &registration.tenant {
            let mut request_headers = HeaderMap::new();
            request_headers.typed_insert(NgsiLdTenant::new(name.clone())); // clone: the header owns the tenant name
            request = request.headers(request_headers);
        }
        if let Some(linked_map) = linked_map {
            let mut map_headers = HeaderMap::new();
            map_headers.typed_insert(NgsiLdEntityMap::new(linked_map.clone())); // clone: the header owns the map identifier
            request = request.headers(map_headers);
        }

        // Clause 6.3.18 requires the existing chain to remain ordered and one
        // local hop to be appended. Header entries own their values, so the
        // validated service-level protocol and alias must be cloned here.
        let new_via = match triggering.via {
            Some(existing) => existing.clone().append(self.received_protocol.clone(), self.source_alias.clone()),
            None => Via::with_entry(self.received_protocol.clone(), self.source_alias.clone()),
        };
        let mut via_headers = HeaderMap::new();
        via_headers.typed_insert(new_via);
        request = request.headers(via_headers);

        Ok(request)
    }
}

/// Add the `contextSourceInfo` pairs outside the reserved keys as custom
/// headers (clause 6.3.19), copying a `urn:ngsi-ld:request` value from the
/// triggering request and never overriding a protected header.
fn apply_custom_headers(
    mut request: reqwest::RequestBuilder,
    info: &ContextSourceInfo<'_>,
    incoming: Option<&HeaderMap>,
) -> Result<reqwest::RequestBuilder, FederationError> {
    for pair in &info.custom {
        let key = pair.key.as_str();
        if protected_header(key) {
            continue;
        }
        let name = HeaderName::from_str(key).map_err(|error| FederationError::internal(format!("invalid contextSourceInfo header name '{key}': {error}")))?;
        if pair.value == FROM_TRIGGERING_REQUEST.as_str() {
            let Some(value) = incoming.and_then(|headers| headers.get(&name)) else {
                continue;
            };
            request = request.header(name, value.clone()); // clone: the forwarded request owns its header value
            continue;
        }
        let value = HeaderValue::from_str(pair.value.as_str())
            .map_err(|error| FederationError::internal(format!("invalid contextSourceInfo value for '{key}': {error}")))?;
        request = request.header(name, value);
    }
    Ok(request)
}

/// Headers derived from the registration itself or set by HTTP, which clause
/// 6.3.19 forbids `contextSourceInfo` from overriding.
fn protected_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "content-length" | "content-type" | "host" | "link" | "ngsild-tenant" | "transfer-encoding" | "via"
    )
}

#[cfg(test)]
mod tests {
    use crate::transport::{apply_custom_headers, protected_header};
    use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, key_value_pair::KeyValuePair};
    use http::{HeaderMap, HeaderValue};

    fn pair(key: &str, value: &str) -> KeyValuePair {
        KeyValuePair {
            key: key.into(),
            value: value.into(),
        }
    }

    fn headers_of(request: reqwest::RequestBuilder) -> HeaderMap {
        request.build().expect("request builds").headers().clone() // clone: the request is dropped with the builder
    }

    #[test]
    fn a_request_valued_pair_is_copied_from_the_triggering_request_and_skipped_without_one() {
        let pairs = [pair("Authorization", "urn:ngsi-ld:request"), pair("X-Static", "fixed")];
        let info = ContextSourceInfo::try_from(pairs.as_slice()).expect("valid info");
        let client = reqwest::Client::new();

        let mut incoming = HeaderMap::new();
        incoming.insert("authorization", HeaderValue::from_static("Bearer token"));
        let forwarded = apply_custom_headers(client.get("http://peer/"), &info, Some(&incoming)).expect("headers apply");
        let headers = headers_of(forwarded);
        pretty_assertions::assert_eq!(headers.get("authorization").map(HeaderValue::as_bytes), Some(b"Bearer token".as_slice()));
        pretty_assertions::assert_eq!(headers.get("x-static").map(HeaderValue::as_bytes), Some(b"fixed".as_slice()));

        let forwarded = apply_custom_headers(client.get("http://peer/"), &info, None).expect("headers apply");
        assert!(headers_of(forwarded).get("authorization").is_none());
    }

    #[test]
    fn protected_headers_cannot_be_overridden_by_context_source_info() {
        for name in ["Via", "NGSILD-Tenant", "content-length", "Link", "Content-Type", "Host"] {
            assert!(protected_header(name), "{name}");
        }
        assert!(!protected_header("Authorization"));
        let pairs = [pair("Via", "1.1 attacker")];
        let info = ContextSourceInfo::try_from(pairs.as_slice()).expect("valid info");
        let forwarded = apply_custom_headers(reqwest::Client::new().get("http://peer/"), &info, None).expect("headers apply");
        assert!(headers_of(forwarded).get("via").is_none());
    }
}
