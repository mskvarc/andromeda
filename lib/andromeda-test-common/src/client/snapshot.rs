//! `/snapshots` endpoint wrappers (NGSI-LD 5.12).

use crate::{
    client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld},
    header::jsonld_link_header,
};
use serde_json::Value;
use urlencoding::encode;

impl NgsiLdClient {
    /// `POST /snapshots`.
    pub async fn create_snapshot(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/snapshots", self.base_url());
        let request = self.apply_tenant(self.http().post(url).header("Content-Type", "application/ld+json").json(body));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /snapshots/{id}`.
    pub async fn retrieve_snapshot(&self, id: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/snapshots/{}", self.base_url(), encode(id));
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /snapshots/{id}`.
    pub async fn delete_snapshot(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/snapshots/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().delete(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
