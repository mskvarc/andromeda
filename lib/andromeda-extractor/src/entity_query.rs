use crate::error::ExtractorRejection;
use axum::{extract::FromRequestParts, http::request::Parts};
use ngsi_ld_query::entity::EntityQueryParameters;

#[derive(Debug)]
pub struct EntityQuery(pub EntityQueryParameters);

impl<S> FromRequestParts<S> for EntityQuery
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let uri = parts.uri.clone();
        // Parse query string manually
        let query_str = parts.uri.query().unwrap_or("");
        let params = serde_urlencoded::from_str::<EntityQueryParameters>(query_str)
            .map_err(|error| ExtractorRejection::invalid_request(format!("Invalid query string: {}", error), uri.clone()))?;

        // Validate options and other parameter values
        params.validate().map_err(|e| ExtractorRejection::from_query_error(e, uri.clone()))?;

        // Validate at least one filter is present, UNLESS local=true
        // Per NGSI-LD spec 5.7.2.4: At least one filter OR local scope must be provided
        let q_is_empty = params.q.as_ref().map(|s| s.trim().is_empty()).unwrap_or(false);

        // Allow empty query if local=true (per NGSI-LD spec 5.7.2.4)
        let has_local_scope = params.local.unwrap_or(false);

        let has_any_filter = params.entity_type.is_some()
            || params.id.is_some()
            || params.id_pattern.is_some()
            || params.attrs.is_some()
            || params.georel.is_some()
            || params.scope_q.is_some()
            || (params.q.is_some() && !q_is_empty);

        // Only require filters if not in local scope
        if !has_local_scope && !has_any_filter {
            return Err(ExtractorRejection::invalid_request(
                "At least one of type, id, idPattern, q, or local=true must be provided",
                uri,
            ));
        }

        Ok(EntityQuery(params))
    }
}

#[allow(dead_code)]
impl EntityQuery {
    pub fn inner(&self) -> &EntityQueryParameters {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn rejects_missing_filters() {
        let req = Request::builder().uri("/ngsi-ld/v1/entities").body(axum::body::Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = EntityQuery::from_request_parts(&mut parts, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn allows_empty_query_with_local_true() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities?local=true")
            .body(axum::body::Body::empty())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = EntityQuery::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok(), "expected Ok when local=true, even without filters");
    }
}
