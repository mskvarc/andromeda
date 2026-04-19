//! `/entityOperations/*` endpoint wrappers (NGSI-LD 5.6.7 – 5.6.13).

use crate::{
    client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld},
    header::jsonld_link_header,
};
use serde_json::Value;

impl NgsiLdClient {
    /// `POST /entityOperations/create` — batch create.
    pub async fn batch_create(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        self.batch("create", body, None).await
    }

    /// `POST /entityOperations/upsert` — batch upsert.
    pub async fn batch_upsert(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        self.batch("upsert", body, None).await
    }

    /// `POST /entityOperations/update` — batch partial update.
    pub async fn batch_update(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        self.batch("update", body, None).await
    }

    /// `POST /entityOperations/merge` — batch merge.
    pub async fn batch_merge(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        self.batch("merge", body, None).await
    }

    /// `POST /entityOperations/delete` — batch delete.
    pub async fn batch_delete(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        self.batch("delete", body, None).await
    }

    /// `POST /entityOperations/query` — POST-based query (NGSI-LD 5.7.2).
    pub async fn batch_query(&self, body: &Value, accept: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entityOperations/query", self.base_url());
        let mut request = self.http().post(url).header("Content-Type", "application/json").header("Accept", accept);
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request.json(body));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    async fn batch(&self, op: &str, body: &Value, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entityOperations/{op}", self.base_url());
        let mut request = self.http().post(url).header("Content-Type", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request.json(body));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
