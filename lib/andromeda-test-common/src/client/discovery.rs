//! Discovery endpoints: `/types`, `/attributes`, `/info`.

use crate::{
    client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld},
    header::jsonld_link_header,
};

impl NgsiLdClient {
    /// `GET /types` — entity-type discovery.
    pub async fn list_types(&self, query: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/types{sep}{query}", self.base_url());
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /attributes` — attribute-name discovery.
    pub async fn list_attributes(&self, query: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/attributes{sep}{query}", self.base_url());
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /info` — broker info endpoint.
    pub async fn info(&self) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/info", self.base_url());
        let request = self.apply_tenant(self.http().get(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /info/sourceIdentity` — Context Source identity endpoint
    /// (NGSI-LD 5.8.1).
    pub async fn source_identity(&self, accept: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/info/sourceIdentity", self.base_url());
        let mut request = self.http().get(url);
        if let Some(accept) = accept {
            request = request.header("Accept", accept);
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
