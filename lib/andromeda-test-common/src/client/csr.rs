//! `/csourceRegistrations` endpoint wrappers (NGSI-LD 5.9).

use crate::{
    client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld},
    header::jsonld_link_header,
};
use serde_json::Value;
use urlencoding::encode;

impl NgsiLdClient {
    /// `POST /csourceRegistrations`.
    pub async fn create_registration(&self, body: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/csourceRegistrations", self.base_url());
        let request = self.apply_tenant(self.http().post(url).header("Content-Type", "application/ld+json").json(body));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /csourceRegistrations/{id}`.
    pub async fn retrieve_registration(&self, id: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/csourceRegistrations/{}", self.base_url(), encode(id));
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /csourceRegistrations`.
    pub async fn query_registrations(&self, query: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/csourceRegistrations{sep}{query}", self.base_url());
        let mut request = self.http().get(url).header("Accept", "application/ld+json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `PATCH /csourceRegistrations/{id}`.
    pub async fn update_registration(&self, id: &str, fragment: &Value, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/csourceRegistrations/{}", self.base_url(), encode(id));
        let mut request = self.http().patch(url).header("Content-Type", "application/json");
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request.json(fragment));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /csourceRegistrations/{id}`.
    pub async fn delete_registration(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/csourceRegistrations/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().delete(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
