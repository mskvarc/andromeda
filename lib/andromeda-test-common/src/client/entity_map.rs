//! `/entityMaps` endpoint wrappers (NGSI-LD 5.7.12).

use crate::client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld};
use urlencoding::encode;

impl NgsiLdClient {
    /// `GET /entityMaps/{id}`.
    pub async fn retrieve_entity_map(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entityMaps/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().get(url).header("Accept", "application/ld+json"));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /entityMaps/{id}`.
    pub async fn delete_entity_map(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entityMaps/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().delete(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
