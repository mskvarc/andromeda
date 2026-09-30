//! HTTP execution and header construction for federated requests.

use crate::{
    FederationService,
    SentResponse,
    cache::{CacheKey, Cacheability, ForwardedGetCache},
    error::FederationError,
    federation_invariant::FederationInvariant,
    forwarded_payload::{ForwardedPayload, build_forwarded_payload},
    forwarding_url::with_local_scope,
    outcome::{ForwardGetOutcome, ForwardedResponse, TransportFailure},
    request::TriggeringRequest,
    source_continuation::SourceContinuation,
};
use andromeda_http_headers::{
    accept::Accept,
    link::header::Link,
    media_type::ResponseMediaType,
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_tenant::NgsiLdTenant,
    ngsild_warning::NgsiLdWarning,
    prefer::Prefer,
    via::Via,
    warning::Warning,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{dereferenceable_uri::DereferenceableUri, identifier::Identifier, spec_version::SpecVersion};
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
    /// The `EntityMap` the registration's context source created for this
    /// query, sent back to it in `NGSILD-EntityMap` (clause 5.5.14).
    pub(crate) linked_map: Option<&'a Identifier>,
}

/// Read a registration's `contextSourceInfo`; stored registrations passed the
/// clause 4.3.6.6 restrictions at registration time, so a failure here is an
/// invariant violation rather than a request error.
fn context_source_info(registration: &ContextSourceRegistration) -> Result<ContextSourceInfo<'_>, FederationError> {
    let pairs = registration.access.context_source_info.as_deref().unwrap_or_default();
    ContextSourceInfo::try_from(pairs).map_err(|source| {
        FederationInvariant::ContextSourceInfo {
            registration: registration.id.clone(), // clone: the error owns the registration it names
            source,
        }
        .into()
    })
}

impl FederationService {
    /// Decide the representation one forwarded request carries for `registration`.
    pub(crate) async fn payload_for(
        &self,
        registration: &ContextSourceRegistration,
        inbound: &ContextSpec,
        body: Option<&[u8]>,
    ) -> Result<ForwardedPayload, FederationError> {
        let info = context_source_info(registration)?;
        build_forwarded_payload(
            &self.engine,
            inbound,
            body,
            &info,
            registration.access.jsonld_context.as_ref().map(DereferenceableUri::url),
        )
        .await
    }

    /// Make a GET request to a context source endpoint.
    pub(crate) async fn forward_get(
        &self,
        registration: &ContextSourceRegistration,
        url: Result<Url, FederationError>,
        inbound: &ContextSpec,
        triggering: TriggeringRequest<'_>,
        cacheability: Cacheability,
        linked_map: Option<&Identifier>,
    ) -> ForwardGetOutcome {
        if self.registration_loops(registration, triggering.via) {
            return ForwardGetOutcome::LoopDetected;
        }
        let url = match url {
            Ok(url) => url,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        let payload = match self.payload_for(registration, inbound, None).await {
            Ok(payload) => payload,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        let cache_duration = match cacheability {
            Cacheability::Cacheable => ForwardedGetCache::duration_of(registration),
            Cacheability::Uncacheable => None,
        };
        let cache_key = cache_duration.map(|_| CacheKey::new(registration, &url, &payload.link_contexts));
        if let Some(key) = &cache_key
            && let Some(response) = self.cache.lookup(key)
        {
            return ForwardGetOutcome::Success(response);
        }
        if let Some(remaining) = self.cooldowns.remaining(registration) {
            // Clause 5.2.34: a request inside the cooldown window is answered
            // with a timeout for the registration, without contacting it.
            return ForwardGetOutcome::Transport(TransportFailure::CoolingDown { remaining });
        }
        debug!("CS forwarding GET {}", url);

        let answered = url.clone(); // clone: the next-page pointer resolves against the URL the request consumes
        let request = match self.build_request(ForwardedRequest {
            method: reqwest::Method::GET,
            url,
            payload: &payload,
            registration,
            triggering,
            linked_map,
        }) {
            Ok(request) => request,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };

        let sent = match self.send_request(registration, request).await {
            Ok(sent) => sent,
            Err(failure) => return ForwardGetOutcome::Transport(failure),
        };

        let outcome = interpret_read_response(registration, &answered, sent);
        if let ForwardGetOutcome::Success(response) = &outcome
            && let (Some(key), Some(duration)) = (cache_key, cache_duration)
        {
            self.cache.store(key, response, duration);
        }
        outcome
    }

    /// Make a POST batch query (`/entityOperations/query`) to a context source
    /// (clause 6.23), sent when a registration advertises `queryBatch` but not a
    /// direct retrieve/query (clause 4.3.6.1). The batch query is not among the
    /// cacheable GET resources of clause 6.3.17, so no cache is consulted, and its
    /// array answer is read exactly as a forwarded query's (clause 5.7.2.4).
    pub(crate) async fn forward_post_query(
        &self,
        registration: &ContextSourceRegistration,
        url: Result<Url, FederationError>,
        body: Vec<u8>,
        inbound: &ContextSpec,
        triggering: TriggeringRequest<'_>,
        linked_map: Option<&Identifier>,
    ) -> ForwardGetOutcome {
        if self.registration_loops(registration, triggering.via) {
            return ForwardGetOutcome::LoopDetected;
        }
        let url = match url {
            Ok(url) => url,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        let payload = match self.payload_for(registration, inbound, Some(&body)).await {
            Ok(payload) => payload,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };
        if let Some(remaining) = self.cooldowns.remaining(registration) {
            // Clause 5.2.34: a request inside the cooldown window is answered
            // with a timeout for the registration, without contacting it.
            return ForwardGetOutcome::Transport(TransportFailure::CoolingDown { remaining });
        }
        debug!("CS forwarding POST {}", url);

        let answered = url.clone(); // clone: the next-page pointer resolves against the URL the request consumes
        let request = match self.build_request(ForwardedRequest {
            method: reqwest::Method::POST,
            url,
            payload: &payload,
            registration,
            triggering,
            linked_map,
        }) {
            Ok(request) => request,
            Err(error) => return ForwardGetOutcome::Transport(TransportFailure::unprepared(error)),
        };

        let sent = match self.send_request(registration, request).await {
            Ok(sent) => sent,
            Err(failure) => return ForwardGetOutcome::Transport(failure),
        };
        interpret_read_response(registration, &answered, sent)
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
        let info = context_source_info(registration)?;

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
        if method == reqwest::Method::GET {
            typed_headers.typed_insert(conformance_preference(&info));
        }
        // Clause 6.3.19 maps the registration's `contentType` instruction to
        // the forwarded request header independently of whether this
        // operation carries a body. Context sources can use that declared
        // representation contract for body-less consumption requests too.
        typed_headers.typed_insert(payload.media_type);

        let mut request = self.http_client.request(method, url).headers(typed_headers);
        request = apply_custom_headers(request, &info, triggering.headers)?;
        if let Some(body) = &payload.body {
            request = request.body(body.clone()); // clone: reqwest owns the body it sends
        }

        // Clause 4.14: "When contacting the respective Context Sources, the
        // Tenant information from the Context Source Registration has to be
        // used. If no Tenant information is present in the Context Source
        // Registration, no Tenant information is to be used and thus the
        // default Tenant is targeted" (Table 5.2.9-1 `tenant`). The tenant the
        // triggering request ran in is never carried over.
        if let Some(tenant) = registration.tenant.clone()
        // clone: the header owns the tenant name
        {
            let tenant = NgsiLdTenant::new(tenant);
            let mut request_headers = HeaderMap::new();
            request_headers.typed_insert(tenant);
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

/// Read a forwarded read response into its clause 6.3.17 outcome: a 404 that
/// contributes nothing, an error status, a stale/relayed warning set, or the
/// parsed entities. The GET and the POST batch-query bindings answer the same
/// way (clause 5.7.2.4), so both interpret their response here. `answered` is
/// the URL the request was sent to, which the source's `next` pointer resolves
/// against (clause 6.3.10).
fn interpret_read_response(registration: &ContextSourceRegistration, answered: &Url, sent: SentResponse) -> ForwardGetOutcome {
    let registration_id = &registration.id;
    if sent.status == reqwest::StatusCode::NOT_FOUND {
        return ForwardGetOutcome::NotFound(sent.ngsild_warnings);
    }
    if !sent.status.is_success() {
        return ForwardGetOutcome::ErrorStatus(sent.status.as_u16());
    }
    if let Some(version) = sent.returned_version {
        // Clause 4.3.6.8: the version the source says its payload conforms
        // to is recorded; the payload is read as the entity data it is.
        debug!(registration = %registration_id, %version, "context source answered with its conformant version");
    }
    // Propagate IANA Warning 110 (Response is Stale) from the context source (clause 6.3.17).
    let mut warnings = Vec::new();
    if let Some(Warning { code: 110, text, .. }) = sent.warning {
        warnings.push(NgsiLdWarning::ResponseIsStale(text));
    }
    // Clause 6.3.17 applies along the whole chain: what the source could not
    // reach behind it is as missing from this response as from its own.
    warnings.extend(sent.ngsild_warnings);
    // Clause 6.3.10: a 206 says where the answer was cut short. One that does
    // not say it cannot be combined with the rest of the result, so the
    // client is told the source's part may be incomplete.
    if sent.status == reqwest::StatusCode::PARTIAL_CONTENT && sent.partial_range.is_none() {
        warnings.push(NgsiLdWarning::MiscellaneousWarning(format!(
            "Context source {registration_id} answered 206 without a readable Content-Range; its part of the history may be incomplete"
        )));
    }

    let text = String::from_utf8_lossy(&sent.body);
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(body) => ForwardGetOutcome::Success(ForwardedResponse {
            body,
            warnings,
            entity_map: sent.entity_map,
            continuation: SourceContinuation::of(sent.next_page.as_ref(), answered, registration.access.endpoint.url()),
            partial_range: sent.partial_range,
        }),
        Err(e) => {
            warn!("Failed to parse CS response as JSON: {}. Body: {}", e, &text[..text.len().min(200)]);
            ForwardGetOutcome::InvalidPayload(e)
        }
    }
}

/// The `Prefer: ngsi-ld=<version>` a forwarded context consumption request
/// carries (clause 4.3.6.8): the version the registration's
/// `ngsildConformance` says the source understands, else the one this broker
/// conforms to, so the source can amend its payload to it.
fn conformance_preference(info: &ContextSourceInfo<'_>) -> Prefer {
    Prefer::version(info.ngsild_conformance.unwrap_or(SpecVersion::CONFORMANT))
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
        let name = HeaderName::from_str(key).map_err(|error| FederationError::from(FederationInvariant::ContextSourceInfoHeaderName(error)))?;
        if pair.value == FROM_TRIGGERING_REQUEST.as_str() {
            let Some(value) = incoming.and_then(|headers| headers.get(&name)) else {
                continue;
            };
            request = request.header(name, value.clone()); // clone: the forwarded request owns its header value
            continue;
        }
        let value =
            HeaderValue::from_str(pair.value.as_str()).map_err(|error| FederationError::from(FederationInvariant::ContextSourceInfoHeaderValue(error)))?;
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
    use crate::{
        FederationService,
        config::FederationConfig,
        fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
        forwarded_payload::ForwardedPayload,
        request::TriggeringRequest,
        transport::{ForwardedRequest, apply_custom_headers, conformance_preference, protected_header},
    };
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
    use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, key_value_pair::KeyValuePair, registration::ContextSourceRegistration};
    use headers::Header;
    use http::{HeaderMap, HeaderValue};
    use serde_json::json;
    use std::sync::Arc;
    use url::Url;

    /// A forwarding service with no registrations of its own, offline.
    fn service() -> FederationService {
        FederationService::new(
            Arc::new(FixedRegistrationStore::new(Vec::new())),
            Arc::new(NoLocalEntities),
            JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"),
            ContextSourcePseudonym::new("andromeda-test").expect("a valid alias"),
            &FederationConfig::default(),
        )
        .expect("the service builds")
    }

    /// A registration naming `tenant`, or no tenant at all.
    fn registration(tenant: Option<&str>) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let Some(tenant) = tenant {
            document["tenant"] = json!(tenant);
        }
        serde_json::from_value(document).expect("valid registration")
    }

    /// The `NGSILD-Tenant` a request forwarded to `registration` carries when
    /// the triggering request ran in the tenant `triggering_tenant`.
    fn forwarded_tenant(registration: &ContextSourceRegistration, triggering_tenant: Option<&'static str>) -> Option<String> {
        let mut incoming = HeaderMap::new();
        if let Some(tenant) = triggering_tenant {
            incoming.insert("ngsild-tenant", HeaderValue::from_static(tenant));
        }
        let payload = ForwardedPayload {
            media_type: ResponseMediaType::Json,
            link_contexts: Vec::new(),
            body: None,
        };
        let request = service()
            .build_request(ForwardedRequest {
                method: reqwest::Method::GET,
                url: Url::parse("https://peer.example.org/ngsi-ld/v1/entities").expect("valid URL"),
                payload: &payload,
                registration,
                triggering: TriggeringRequest {
                    via: None,
                    headers: Some(&incoming),
                },
                linked_map: None,
            })
            .expect("the request builds");
        headers_of(request)
            .get("ngsild-tenant")
            .map(|value| value.to_str().expect("a valid header value").to_owned())
    }

    /// Clause 4.14 and Table 5.2.9-1: the forwarded request targets the tenant
    /// the registration names, whatever tenant the triggering request ran in.
    #[test]
    fn a_registration_tenant_is_the_forwarded_tenant() {
        let registration = registration(Some("domain-tenant"));
        pretty_assertions::assert_eq!(forwarded_tenant(&registration, None).as_deref(), Some("domain-tenant"));
        pretty_assertions::assert_eq!(forwarded_tenant(&registration, Some("client-tenant")).as_deref(), Some("domain-tenant"));
    }

    /// Clause 4.14: "If no Tenant information is present in the Context Source
    /// Registration, no Tenant information is to be used and thus the default
    /// Tenant is targeted" -- the triggering request's tenant is not inherited.
    #[test]
    fn a_registration_without_a_tenant_targets_the_default_tenant() {
        let registration = registration(None);
        pretty_assertions::assert_eq!(forwarded_tenant(&registration, None), None);
        pretty_assertions::assert_eq!(forwarded_tenant(&registration, Some("client-tenant")), None);
    }

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

    /// Clause 4.3.6.8: a forwarded GET asks for the version the registration
    /// declares its source understands, and for this broker's own version when
    /// it declares none.
    #[test]
    fn the_conformance_preference_names_the_registered_version_or_the_conformant_one() {
        let declared = [pair("ngsildConformance", "1.5")];
        let info = ContextSourceInfo::try_from(declared.as_slice()).expect("valid info");
        let mut encoded = Vec::new();
        conformance_preference(&info).encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded[0].to_str().expect("valid str"), "ngsi-ld=1.5");

        let undeclared: [KeyValuePair; 0] = [];
        let info = ContextSourceInfo::try_from(undeclared.as_slice()).expect("valid info");
        let mut encoded = Vec::new();
        conformance_preference(&info).encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded[0].to_str().expect("valid str"), "ngsi-ld=1.9");
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
