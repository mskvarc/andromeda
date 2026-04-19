//! `/jsonldContexts` endpoint wrappers (NGSI-LD 5.13).

use crate::client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld};
use serde_json::Value;
use urlencoding::encode;

impl NgsiLdClient {
    /// `POST /jsonldContexts` — register an inline `@context`.
    pub async fn create_jsonld_context(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/jsonldContexts", self.base_url());
        let request = self.apply_tenant(self.http().post(url).header("Content-Type", "application/ld+json").json(body));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /jsonldContexts/{id}`.
    pub async fn retrieve_jsonld_context(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/jsonldContexts/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().get(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /jsonldContexts/{id}`.
    pub async fn delete_jsonld_context(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/jsonldContexts/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().delete(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
