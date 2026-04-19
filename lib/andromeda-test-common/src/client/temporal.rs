//! `/temporal/entities` endpoint wrappers (NGSI-LD 5.7.5 – 5.7.9).

use crate::{
    client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld},
    header::jsonld_link_header,
};
use serde_json::Value;
use urlencoding::encode;

impl NgsiLdClient {
    /// `POST /temporal/entities`.
    pub async fn create_temporal_entity(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/temporal/entities", self.base_url());
        let request = self.apply_tenant(self.http().post(url).header("Content-Type", "application/ld+json").json(body));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /temporal/entities/{id}` with query string `query`.
    pub async fn get_temporal_entity(&self, id: &str, query: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/temporal/entities/{}{sep}{query}", self.base_url(), encode(id));
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /temporal/entities`.
    pub async fn query_temporal_entities(&self, query: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/temporal/entities{sep}{query}", self.base_url());
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /temporal/entities/{id}`.
    pub async fn delete_temporal_entity(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/temporal/entities/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().delete(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
