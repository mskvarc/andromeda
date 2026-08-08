//! Shared HTTP execution and header construction for forwarded requests.

use crate::{
    error::AndromedaError,
    service::csource_forwarding::{CsourceForwardingService, outcome::ForwardGetOutcome},
};
use andromeda_ngsi_ld_http_headers::{
    context_source_pseudonym::ContextSourcePseudonym,
    link::header::Link,
    ngsild_tenant::NgsiLdTenant,
    ngsild_warning::NgsiLdWarning,
    received_protocol::ReceivedProtocol,
    via::Via,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use headers::Header;
use iri_rs::IriRefBuf;
use tracing::{debug, warn};
use url::Url;

impl CsourceForwardingService {
    /// Make a GET request to a context source endpoint.
    pub(crate) async fn forward_get(
        &self,
        url: Result<Url, AndromedaError>,
        context_url: Option<&str>,
        tenant: Option<&TenantName>,
        incoming_via: Option<&Via>,
    ) -> ForwardGetOutcome {
        let url = match url {
            Ok(url) => url,
            Err(error) => return ForwardGetOutcome::NetworkError(error.to_string()),
        };
        debug!("CS forwarding GET {}", url);

        let request = self
            .build_request(reqwest::Method::GET, url.as_str(), context_url, tenant, incoming_via)
            .header("Accept", "application/json");

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
        if let Some(ref warning_value) = warning_header
            && warning_value.starts_with("110")
        {
            warnings.push(NgsiLdWarning::ResponseIsStale(warning_value.clone()));
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
        url: &str,
        context_url: Option<&str>,
        tenant: Option<&TenantName>,
        incoming_via: Option<&Via>,
    ) -> reqwest::RequestBuilder {
        let mut request = self.http_client.request(method, url).header("Connection", "close");

        if let Some(ctx) = context_url
            && let Ok(context_iri) = IriRefBuf::new(ctx.to_string())
        {
            let link = Link::jsonld_context(context_iri);
            if let Some(value) = link.to_header_value() {
                request = request.header("Link", value);
            }
        }

        if let Some(name) = tenant {
            let tenant_header = NgsiLdTenant::new(name.clone()); // clone: TenantName is borrowed, header needs ownership
            request = request.header(NgsiLdTenant::name().as_str(), tenant_header.as_str());
        }

        // Append our identity to the Via header chain (section 6.3.18). The
        // protocol token and our identity are both fixed for a valid deployment,
        // so this construction cannot fail in practice; were either malformed the
        // chain is forwarded without our hop rather than the request aborted.
        if let (Ok(protocol), Ok(pseudonym)) = (
            ReceivedProtocol::try_from("1.1".to_string()),
            ContextSourcePseudonym::try_from(self.source_identity.clone()), // clone: the pseudonym owns its string
        ) {
            let new_via = match incoming_via {
                Some(existing) => existing.clone().append(protocol, pseudonym), // clone: need owned Via to append our identity
                None => Via::with_entry(protocol, pseudonym),
            };
            let mut via_values = Vec::new();
            headers::Header::encode(&new_via, &mut via_values);
            if let Some(val) = via_values.into_iter().next() {
                request = request.header("via", val);
            }
        }

        request
    }
}
