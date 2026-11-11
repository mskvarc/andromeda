//! The query-string binding of Query Entities (clause 6.4.3.2): the query a
//! client wrote, or a followed pointer naming a query a client sent by POST.

use crate::{
    entity_query::EntityQuery,
    posted_reference::{decode_posted_reference, names_posted_query},
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_request::{
    entity_operations::PostQueryParams,
    pagination::posted_reference::PostedQueryReference,
    query::entity::parameters::EntityQueryParameters,
};
use axum::{extract::FromRequestParts, http::request::Parts};

/// What the query string of `GET /entities` names.
#[derive(Debug)]
pub enum EntityQuerySource {
    /// The query itself.
    Query(Box<EntityQueryParameters>),
    /// A posted query the broker holds, by its key, beside the URL parameters
    /// of the POST that posted it (clause 6.23.3.1).
    Posted(PostedQueryReference<PostQueryParams>),
}

impl<S> FromRequestParts<S> for EntityQuerySource
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<EntityQuerySource, ExtractorRejection> {
        if names_posted_query::<PostQueryParams>(parts) {
            return decode_posted_reference(parts).map(EntityQuerySource::Posted);
        }
        let EntityQuery(query) = EntityQuery::from_request_parts(parts, state).await?;
        Ok(EntityQuerySource::Query(Box::new(query)))
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_query_source::EntityQuerySource;
    use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
    use axum::{extract::FromRequestParts, http::Request};

    async fn source(query: &str) -> EntityQuerySource {
        let (mut parts, ()) = Request::builder()
            .uri(format!("/ngsi-ld/v1/entities?{query}"))
            .body(())
            .expect("a valid request")
            .into_parts();
        EntityQuerySource::from_request_parts(&mut parts, &()).await.expect("extracts")
    }

    #[tokio::test]
    async fn a_query_string_naming_a_posted_query_is_a_reference() {
        let key = PageQuery::of(Some("type=Vehicle"));
        let posted = source(&format!("limit=2&postedQuery={}", key.encode())).await;
        assert!(matches!(posted, EntityQuerySource::Posted(reference) if reference.key == key));
        assert!(matches!(source("type=Vehicle&limit=2").await, EntityQuerySource::Query(_)));
    }
}
