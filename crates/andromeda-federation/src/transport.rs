//! HTTP execution and header construction for federated requests.

use crate::{FederationService, error::FederationError, outcome::ForwardGetOutcome};
use andromeda_http_headers::{
    accept::Accept,
    link::header::Link,
    media_type::ResponseMediaType,
    ngsild_tenant::NgsiLdTenant,
    ngsild_warning::NgsiLdWarning,
    via::Via,
    warning::Warning,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, registration::ContextSourceAccess};
use headers::{Header, HeaderMapExt};
use iri_rs::IriRefBuf;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::{iter, str::FromStr};
use tracing::{debug, warn};
use url::Url;

impl FederationService {
    /// Make a GET request to a context source endpoint.
    pub(crate) async fn forward_get(
        &self,
        url: Result<Url, FederationError>,
        context_url: Option<&IriRefBuf>,
        access: &ContextSourceAccess,
        tenant: Option<&TenantName>,
        incoming_via: Option<&Via>,
    ) -> ForwardGetOutcome {
        let url = match url {
            Ok(url) => url,
            Err(error) => return ForwardGetOutcome::NetworkError(error.to_string()),
        };
        debug!("CS forwarding GET {}", url);

        let request = match self.build_request(reqwest::Method::GET, &url, context_url, access, tenant, incoming_via) {
            Ok(request) => request,
            Err(error) => return ForwardGetOutcome::NetworkError(error.to_string()),
        };

        let (status, warning_header, bytes) = match self.send_request(request).await {
            Ok(result) => result,
            Err(e) => return ForwardGetOutcome::NetworkError(e.to_string()),
        };

        if status.as_u16() == 404 {
            return ForwardGetOutcome::NotFound;
        }

        if !status.is_success() {
            return ForwardGetOutcome::ErrorStatus(status.as_u16());
        }

        // Propagate IANA Warning 110 (Response is Stale) from the context source (clause 6.3.17).
        let mut warnings = Vec::new();
        if let Some(Warning { code: 110, text, .. }) = warning_header {
            warnings.push(NgsiLdWarning::ResponseIsStale(text));
        }

        let text = String::from_utf8_lossy(&bytes);
        match serde_json::from_str(&text) {
            Ok(value) => ForwardGetOutcome::Success(value, warnings),
            Err(e) => {
                warn!("Failed to parse CS response as JSON: {}. Body: {}", e, &text[..text.len().min(200)]);
                ForwardGetOutcome::InvalidPayload(format!("Failed to parse context source response as JSON: {}", e))
            }
        }
    }

    /// Build a request with shared headers (Connection: close, Link context, Via).
    pub(crate) fn build_request(
        &self,
        method: reqwest::Method,
        url: &Url,
        context_url: Option<&IriRefBuf>,
        access: &ContextSourceAccess,
        tenant: Option<&TenantName>,
        incoming_via: Option<&Via>,
    ) -> Result<reqwest::RequestBuilder, FederationError> {
        let mut typed_headers = HeaderMap::new();
        typed_headers.typed_insert(headers::Connection::close());

        let pairs = access.context_source_info.as_deref().unwrap_or_default();
        let configured_context = pair_value(pairs, "jsonldContext").or_else(|| access.jsonld_context.as_ref().map(Url::as_str));
        let configured_context = configured_context
            .map(IriRefBuf::from_str)
            .transpose()
            .map_err(|error| FederationError::internal(format!("invalid contextSourceInfo jsonldContext: {error}")))?;
        let effective_context = configured_context.as_ref().or(context_url);
        if let Some(context_iri) = effective_context {
            let link = Link::jsonld_context(context_iri.clone()); // clone: the header owns the validated IRI reference.
            if let Some(value) = link.to_header_value() {
                typed_headers.insert(Link::name().clone(), value);
            }
        }

        let accept_value = pair_value(pairs, "accept")
            .map(str::to_owned)
            .or_else(|| access.accept.as_ref().map(ToString::to_string));
        let accept = accept_value
            .as_deref()
            .and_then(|raw| {
                let value = HeaderValue::from_str(raw).ok()?;
                let mut values = iter::once(&value);
                Accept::decode(&mut values).ok()
            })
            .unwrap_or_else(|| Accept::single(ResponseMediaType::Json));
        typed_headers.typed_insert(accept);
        if let Some(content_type) = forwarded_content_type(access) {
            typed_headers.typed_insert(content_type);
        }
        let mut request = apply_custom_headers(self.http_client.request(method, url.clone()).headers(typed_headers), pairs)?;

        if let Some(name) = tenant {
            let tenant_header = NgsiLdTenant::new(name.clone()); // clone: TenantName is borrowed, header needs ownership
            let mut request_headers = HeaderMap::new();
            request_headers.typed_insert(tenant_header);
            request = request.headers(request_headers);
        }

        // Clause 6.3.18 requires the existing chain to remain ordered and one
        // local hop to be appended. Header entries own their values, so the
        // validated service-level protocol and alias must be cloned here.
        let new_via = match incoming_via {
            Some(existing) => existing.clone().append(self.received_protocol.clone(), self.source_alias.clone()),
            None => Via::with_entry(self.received_protocol.clone(), self.source_alias.clone()),
        };
        let mut via_headers = HeaderMap::new();
        via_headers.typed_insert(new_via);
        request = request.headers(via_headers);

        Ok(request)
    }
}

pub(crate) fn forwarded_content_type(access: &ContextSourceAccess) -> Option<ResponseMediaType> {
    let pairs = access.context_source_info.as_deref().unwrap_or_default();
    pair_value(pairs, "contentType")
        .and_then(|value| value.parse::<mediatype::MediaTypeBuf>().ok())
        .and_then(|value| {
            let parsed: mediatype::MediaType<'_> = (&value).into();
            ResponseMediaType::from_media_type(&parsed)
        })
        .or_else(|| {
            if pair_value(pairs, "jsonldContext").is_some() || access.jsonld_context.is_some() {
                Some(ResponseMediaType::Json)
            } else {
                None
            }
        })
}

fn pair_value<'a>(pairs: &'a [KeyValuePair], key: &str) -> Option<&'a str> {
    pairs.iter().find(|pair| pair.key == key).map(|pair| pair.value.as_str())
}

fn apply_custom_headers(mut request: reqwest::RequestBuilder, pairs: &[KeyValuePair]) -> Result<reqwest::RequestBuilder, FederationError> {
    for pair in pairs {
        let key = pair.key.as_str();
        if matches!(key, "accept" | "contentType" | "jsonldContext") || pair.value == "urn:ngsi-ld:request" {
            continue;
        }
        if protected_header(key) {
            continue;
        }
        let name = HeaderName::from_str(key).map_err(|error| FederationError::internal(format!("invalid contextSourceInfo header name '{key}': {error}")))?;
        let value = HeaderValue::from_str(pair.value.as_str())
            .map_err(|error| FederationError::internal(format!("invalid contextSourceInfo value for '{key}': {error}")))?;
        request = request.header(name, value);
    }
    Ok(request)
}

fn protected_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "content-length" | "host" | "ngsild-tenant" | "transfer-encoding" | "via"
    )
}
