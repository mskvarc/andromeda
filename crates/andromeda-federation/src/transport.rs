//! Header construction for federated requests (clauses 6.3.18, 6.3.19).

use crate::{
    FederationService,
    error::FederationError,
    forwarded_payload::ForwardedPayload,
    forwarding_url::with_local_scope,
    registration_payload::context_source_info,
    request::TriggeringRequest,
};
use andromeda_http_headers::{
    accept::Accept,
    link::header::Link,
    media_type::ResponseMediaType,
    ngsild_entity_map::NgsiLdEntityMap,
    ngsild_tenant::NgsiLdTenant,
    prefer::Prefer,
    via::Via,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, registration::ContextSourceRegistration, registration_management::LocalScope};
use headers::{Header, HeaderMapExt};
use http::{HeaderMap, HeaderName, HeaderValue};
use iri_rs::{Iri, iri};
use std::iter;
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

impl FederationService {
    /// Build a request with every header clause 6.3.19 and clause 6.3.18 call
    /// for: the one context `Link`, `Accept`, the body with its `Content-Type`,
    /// the `contextSourceInfo` custom headers, `NGSILD-Tenant` and `Via`.
    pub(crate) fn build_request(&self, forwarded: ForwardedRequest<'_>) -> Result<reqwest::RequestBuilder, FederationError> {
        // Clause 5.5.13 through clause 6.3.18: a `localOnly` registration is
        // asked for its own data only.
        let url = match forwarded.registration.access.management.as_ref().and_then(|management| management.local_only) {
            Some(LocalScope::LocalOnly) => with_local_scope(forwarded.url),
            Some(LocalScope::Cascading) | None => forwarded.url,
        };
        self.build_unscoped_request(ForwardedRequest { url, ..forwarded })
    }

    /// Build a request exactly as [`Self::build_request`] does, except that
    /// the URL is sent as given: a request about the Context Source itself
    /// rather than about its data (clause 5.15.1 defines no parameter at all)
    /// carries no local scope.
    pub(crate) fn build_unscoped_request(&self, forwarded: ForwardedRequest<'_>) -> Result<reqwest::RequestBuilder, FederationError> {
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

        let mut typed_headers = HeaderMap::new();
        typed_headers.typed_insert(headers::Connection::close());
        // JSON-LD 1.1 section 6.1 admits one context link, and a source that
        // follows the JSON-LD 1.1 API refuses a request naming several.
        if let Some(context) = &payload.link_context {
            Link::insert_jsonld_context(&mut typed_headers, context.clone()); // clone: the header owns the context IRI while the payload is sent again for the next registration.
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
        // Clause 6.3.19 sets `Content-Type` "whenever a payload body is
        // present": a body-less request carries none, which also keeps an
        // `application/ld+json` media type from travelling beside a context
        // `Link` a source must refuse under clause 6.3.5.
        if payload.body.is_some() {
            typed_headers.typed_insert(payload.media_type);
        }

        let mut request = self.http_client.request(method, url).headers(typed_headers);
        request = apply_custom_headers(request, &info, triggering.headers);
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
            Some(existing) => existing.clone().append(self.received_protocol.clone(), self.local.alias().clone()),
            None => Via::with_entry(self.received_protocol.clone(), self.local.alias().clone()),
        };
        let mut via_headers = HeaderMap::new();
        via_headers.typed_insert(new_via);
        request = request.headers(via_headers);

        Ok(request)
    }
}

/// The `Prefer: ngsi-ld=<version>` a forwarded context consumption request
/// carries (clause 4.3.6.8): the version the registration's
/// `ngsildConformance` says the source understands, else the one this broker
/// conforms to, so the source can amend its payload to it.
fn conformance_preference(info: &ContextSourceInfo) -> Prefer {
    Prefer::version(info.ngsild_conformance.unwrap_or(SpecVersion::CONFORMANT))
}

/// Add the `contextSourceInfo` pairs outside the reserved keys as custom
/// headers (clause 6.3.19), copying a `urn:ngsi-ld:request` value from the
/// triggering request and never overriding a protected header.
fn apply_custom_headers(mut request: reqwest::RequestBuilder, info: &ContextSourceInfo, incoming: Option<&HeaderMap>) -> reqwest::RequestBuilder {
    for field in &info.custom {
        if protected_header(&field.name) {
            continue;
        }
        if field.value == FROM_TRIGGERING_REQUEST.as_str() {
            let Some(value) = incoming.and_then(|headers| headers.get(&field.name)) else {
                continue;
            };
            request = request.header(field.name.clone(), value.clone()); // clone: the forwarded request owns its header value
            continue;
        }
        request = request.header(field.name.clone(), field.value.clone()); // clone: the forwarded request owns its header value
    }
    request
}

/// Headers derived from the registration itself or set by HTTP, which clause
/// 6.3.19 forbids `contextSourceInfo` from overriding.
fn protected_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection" | "content-length" | "content-type" | "host" | "link" | "ngsild-tenant" | "transfer-encoding" | "via"
    )
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        fixed_registration_store::offline_service,
        forwarded_payload::ForwardedPayload,
        request::TriggeringRequest,
        transport::{ForwardedRequest, apply_custom_headers, conformance_preference, protected_header},
    };
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, key_value_pair::KeyValuePair, registration::ContextSourceRegistration};
    use headers::Header;
    use http::{HeaderMap, HeaderName, HeaderValue};
    use iri_rs::IriBuf;
    use serde_json::json;
    use url::Url;

    /// A registration naming `tenant`, or no tenant at all.
    fn registration(tenant: Option<&str>) -> ContextSourceRegistration {
        registration_with_info(tenant, &[])
    }

    /// A registration naming `tenant`, or no tenant at all, whose
    /// `contextSourceInfo` holds `pairs`.
    fn registration_with_info(tenant: Option<&str>, pairs: &[(&str, &str)]) -> ContextSourceRegistration {
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
        if !pairs.is_empty() {
            document["contextSourceInfo"] = pairs.iter().map(|(key, value)| json!({"key": key, "value": value})).collect();
        }
        serde_json::from_value(document).expect("valid registration")
    }

    /// The headers of `payload` forwarded with `method` to `registration`, when
    /// the triggering request carried `incoming`.
    fn forwarded_headers(registration: &ContextSourceRegistration, method: reqwest::Method, payload: &ForwardedPayload, incoming: &HeaderMap) -> HeaderMap {
        let request = offline_service(Vec::new())
            .build_request(ForwardedRequest {
                method,
                url: Url::parse("https://peer.example.org/ngsi-ld/v1/entities").expect("valid URL"),
                payload,
                registration,
                triggering: TriggeringRequest {
                    via: None,
                    headers: Some(incoming),
                },
                linked_map: None,
            })
            .expect("the request builds");
        headers_of(request)
    }

    /// The headers of a body-less GET forwarded to `registration` when the
    /// triggering request ran in the tenant `triggering_tenant`.
    fn forwarded_get_headers(registration: &ContextSourceRegistration, triggering_tenant: Option<&'static str>) -> HeaderMap {
        let mut incoming = HeaderMap::new();
        if let Some(tenant) = triggering_tenant {
            incoming.insert("ngsild-tenant", HeaderValue::from_static(tenant));
        }
        let payload = ForwardedPayload {
            media_type: ResponseMediaType::Json,
            link_context: None,
            body: None,
        };
        forwarded_headers(registration, reqwest::Method::GET, &payload, &incoming)
    }

    /// The `NGSILD-Tenant` values a request forwarded to `registration`
    /// carries when the triggering request ran in the tenant
    /// `triggering_tenant`.
    fn forwarded_tenants(registration: &ContextSourceRegistration, triggering_tenant: Option<&'static str>) -> Vec<String> {
        forwarded_get_headers(registration, triggering_tenant)
            .get_all("ngsild-tenant")
            .iter()
            .map(|value| value.to_str().expect("a valid header value").to_owned())
            .collect()
    }

    /// The one `NGSILD-Tenant` a request forwarded to `registration` carries,
    /// if any.
    fn forwarded_tenant(registration: &ContextSourceRegistration, triggering_tenant: Option<&'static str>) -> Option<String> {
        let tenants = forwarded_tenants(registration, triggering_tenant);
        assert!(tenants.len() <= 1, "at most one tenant travels: {tenants:?}");
        tenants.into_iter().next()
    }

    /// Every JSON-LD context link-value across all `Link` field lines.
    fn context_link_values(headers: &HeaderMap) -> Vec<String> {
        headers
            .get_all("link")
            .iter()
            .flat_map(|line| line.to_str().expect("an ASCII Link header").split(',').map(str::to_owned).collect::<Vec<_>>())
            .filter(|value| value.contains("http://www.w3.org/ns/json-ld#context"))
            .collect()
    }

    /// JSON-LD 1.1 section 6.1: a forwarded request names exactly one context
    /// link-value when its payload has a context `Link`, and none otherwise.
    #[test]
    fn a_forwarded_request_carries_exactly_the_one_context_link_of_its_payload() {
        let registration = registration(None);
        let with_link = ForwardedPayload {
            media_type: ResponseMediaType::Json,
            link_context: Some(IriBuf::new("https://example.org/user.jsonld".to_owned()).expect("valid IRI")),
            body: None,
        };
        let values = context_link_values(&forwarded_headers(&registration, reqwest::Method::GET, &with_link, &HeaderMap::new()));
        pretty_assertions::assert_eq!(values.len(), 1, "{values:?}");
        assert!(values[0].contains("<https://example.org/user.jsonld>"), "{values:?}");

        let without_link = ForwardedPayload {
            media_type: ResponseMediaType::Json,
            link_context: None,
            body: None,
        };
        pretty_assertions::assert_eq!(
            context_link_values(&forwarded_headers(&registration, reqwest::Method::GET, &without_link, &HeaderMap::new())),
            Vec::<String>::new()
        );
    }

    /// Clause 6.3.19 sets `Content-Type` "whenever a payload body is present":
    /// a body-less request carries none, even when the registration's
    /// `contentType` is `application/ld+json`.
    #[test]
    fn content_type_is_sent_only_with_a_body() {
        let registration = registration(None);
        let body_less = ForwardedPayload {
            media_type: ResponseMediaType::JsonLd,
            link_context: None,
            body: None,
        };
        for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
            assert!(
                forwarded_headers(&registration, method, &body_less, &HeaderMap::new())
                    .get("content-type")
                    .is_none()
            );
        }

        let with_body = ForwardedPayload {
            media_type: ResponseMediaType::JsonLd,
            link_context: None,
            body: Some(b"{}".to_vec()),
        };
        let headers = forwarded_headers(&registration, reqwest::Method::POST, &with_body, &HeaderMap::new());
        pretty_assertions::assert_eq!(headers.get("content-type").map(HeaderValue::as_bytes), Some(b"application/ld+json".as_slice()));
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

    /// Clause 4.3.6.5: tenant information and binding-specific headers in
    /// `contextSourceInfo` "shall be ignored", and clause 4.14 takes the
    /// tenant from the registration's `tenant` member, so neither entry
    /// reaches the source.
    #[test]
    fn tenant_and_content_length_entries_of_context_source_info_never_reach_the_source() {
        let entries = [("NGSILD-Tenant", "info-tenant"), ("Content-Length", "99")];
        for (tenant, expected) in [(Some("domain-tenant"), vec!["domain-tenant".to_owned()]), (None, Vec::new())] {
            let registration = registration_with_info(tenant, &entries);
            pretty_assertions::assert_eq!(forwarded_tenants(&registration, Some("client-tenant")), expected, "{tenant:?}");
            let headers = forwarded_get_headers(&registration, Some("client-tenant"));
            assert!(headers.get("content-length").is_none(), "{tenant:?}: {headers:?}");
        }
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
        let forwarded = apply_custom_headers(client.get("http://peer/"), &info, Some(&incoming));
        let headers = headers_of(forwarded);
        pretty_assertions::assert_eq!(headers.get("authorization").map(HeaderValue::as_bytes), Some(b"Bearer token".as_slice()));
        pretty_assertions::assert_eq!(headers.get("x-static").map(HeaderValue::as_bytes), Some(b"fixed".as_slice()));

        let forwarded = apply_custom_headers(client.get("http://peer/"), &info, None);
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
        for name in ["via", "ngsild-tenant", "content-length", "link", "content-type", "host"] {
            assert!(protected_header(&HeaderName::from_static(name)), "{name}");
        }
        assert!(!protected_header(&HeaderName::from_static("authorization")));
        let pairs = [pair("Via", "1.1 attacker"), pair("Content-Length", "99"), pair("NGSILD-Tenant", "t1")];
        let info = ContextSourceInfo::try_from(pairs.as_slice()).expect("valid info");
        let forwarded = apply_custom_headers(reqwest::Client::new().get("http://peer/"), &info, None);
        let headers = headers_of(forwarded);
        assert!(headers.get("via").is_none());
        assert!(headers.get("content-length").is_none());
        assert!(headers.get("ngsild-tenant").is_none());
    }
}
