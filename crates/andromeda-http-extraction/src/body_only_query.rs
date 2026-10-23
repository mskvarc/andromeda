//! The clause 5.2.23 Query body of an operation that defines no URL parameter.
//!
//! The entity map creations via POST (clauses 6.34.3.2 and 6.35.3.2) carry
//! their whole query in the body, and neither table names a URL parameter, so
//! any parameter on the URL is the clause 6.3.20 `InvalidRequest`. The URL is
//! read before the body, so a refused parameter costs no body decoding.

use crate::{jsonld::JsonLd, query::ValidatedQuery};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_request::empty::EmptyParams;
use axum::extract::{FromRequest, FromRequestParts, Request};

/// A Query body, admitted only when the URL carries no parameter.
pub struct BodyOnlyQuery(pub JsonLd<Query>);

impl<S> FromRequest<S> for BodyOnlyQuery
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request(request: Request, state: &S) -> Result<BodyOnlyQuery, ExtractorRejection> {
        let (mut parts, body) = request.into_parts();
        let ValidatedQuery(EmptyParams {}) = ValidatedQuery::<EmptyParams>::from_request_parts(&mut parts, state).await?;
        Ok(BodyOnlyQuery(JsonLd::<Query>::from_request(Request::from_parts(parts, body), state).await?))
    }
}

#[cfg(test)]
mod tests {
    use crate::body_only_query::BodyOnlyQuery;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{
        body::Body,
        extract::FromRequest,
        http::{
            Request,
            header::{CONTENT_LENGTH, CONTENT_TYPE},
        },
    };

    const QUERY: &str = r#"{"type": "Query", "entities": [{"type": "Vehicle"}]}"#;

    fn request(uri: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, QUERY.len())
            .body(Body::from(QUERY))
            .expect("the request builds")
    }

    #[tokio::test]
    async fn a_query_body_without_url_parameters_is_read() {
        assert!(BodyOnlyQuery::from_request(request("/ngsi-ld/v1/entityMaps"), &()).await.is_ok());
    }

    /// Clauses 6.34.3.2 and 6.35.3.2 define no URL parameter, so one beside
    /// the body is the clause 6.3.20 `InvalidRequest`.
    #[tokio::test]
    async fn a_url_parameter_beside_the_body_is_invalid_request() {
        let rejection = BodyOnlyQuery::from_request(request("/ngsi-ld/v1/entityMaps?limit=3"), &())
            .await
            .err()
            .expect("a URL parameter is refused");
        let problem = rejection.problem().expect("a classified rejection");
        pretty_assertions::assert_eq!(problem.error_type(), NgsiLdErrorType::InvalidRequest);
        assert!(problem.detail().contains("limit"), "{}", problem.detail());
    }
}
