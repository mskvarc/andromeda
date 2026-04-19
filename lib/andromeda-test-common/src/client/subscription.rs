//! `/subscriptions` endpoint wrappers.

use crate::{
    client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld},
    header::jsonld_link_header,
};
use serde_json::Value;
use urlencoding::encode;

impl NgsiLdClient {
    /// `POST /subscriptions`.
    pub async fn create_subscription(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/subscriptions", self.base_url());
        let request = self.apply_tenant(self.http().post(url).header("Content-Type", "application/ld+json").json(body));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /subscriptions/{id}`.
    pub async fn retrieve_subscription(&self, id: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/subscriptions/{}", self.base_url(), encode(id));
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `PATCH /subscriptions/{id}` — partial update (NGSI-LD 5.8.4).
    pub async fn update_subscription(&self, id: &str, fragment: &Value, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/subscriptions/{}", self.base_url(), encode(id));
        let mut request = self.http().patch(url).header("Content-Type", "application/json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request.json(fragment));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /subscriptions/{id}`.
    pub async fn delete_subscription(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/subscriptions/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().delete(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /subscriptions` — query subscriptions (NGSI-LD 5.8.5).
    pub async fn query_subscriptions(&self, query: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/subscriptions{sep}{query}", self.base_url());
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
