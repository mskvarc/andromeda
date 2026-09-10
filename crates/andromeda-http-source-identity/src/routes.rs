//! Typed, state-complete source-identity route inventory.

use crate::{handler, state::SourceIdentityState};
use axum::{Router, routing::get};

/// The complete source-identity route fragment.
pub struct SourceIdentityRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl SourceIdentityRoutes {
    /// Construct `GET /info/sourceIdentity` with focused immutable state.
    pub fn new(state: SourceIdentityState) -> SourceIdentityRoutes {
        let router = Router::new().route("/sourceIdentity", get(handler::get)).with_state(state);
        SourceIdentityRoutes {
            router: Router::new().nest("/info", router),
        }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::{routes::SourceIdentityRoutes, state::SourceIdentityState};
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
    use andromeda_tenancy::context_source_alias::ContextSourceAliases;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, Response, StatusCode},
    };
    use chrono::Utc;
    use serde_json::Value;
    use std::error::Error;
    use tower::ServiceExt;

    /// The identity document the fragment answers `tenant` with.
    async fn identity_for(tenant: Option<&str>) -> Result<(StatusCode, Value), Box<dyn Error>> {
        let state = SourceIdentityState::new(
            "urn:ngsi-ld:ContextSourceIdentity:test".parse::<Uri>()?,
            ContextSourceAliases::new("test".parse::<ContextSourcePseudonym>()?),
            Utc::now(),
            None,
        );
        let mut request = Request::get("/info/sourceIdentity");
        if let Some(tenant) = tenant {
            request = request.header("NGSILD-Tenant", tenant);
        }
        let response: Response<Body> = SourceIdentityRoutes::new(state).into_router().oneshot(request.body(Body::empty())?).await?;
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        Ok((status, serde_json::from_slice(&body)?))
    }

    #[tokio::test]
    async fn source_identity_fragment_owns_its_complete_path() -> Result<(), Box<dyn Error>> {
        let (status, _) = identity_for(None).await?;
        pretty_assertions::assert_eq!(status, StatusCode::OK);
        Ok(())
    }

    /// Table 5.2.40-1: the alias identifies the Tenant the request addressed,
    /// so the route reads the clause 6.3.14 header rather than ignoring it.
    #[tokio::test]
    async fn the_route_answers_each_tenant_with_its_own_alias() -> Result<(), Box<dyn Error>> {
        let (_, default) = identity_for(None).await?;
        let (_, alpha) = identity_for(Some("alpha")).await?;
        pretty_assertions::assert_eq!(default["contextSourceAlias"], Value::from("test"));
        pretty_assertions::assert_eq!(alpha["contextSourceAlias"], Value::from("test~alpha"));
        pretty_assertions::assert_eq!(default["id"], alpha["id"]);
        Ok(())
    }

    /// Clause 6.3.14 makes an empty `NGSILD-Tenant` no tenant at all, and the
    /// shared extractor answers it with the same rejection every route uses.
    #[tokio::test]
    async fn an_empty_tenant_header_is_rejected_rather_than_read_as_the_default() -> Result<(), Box<dyn Error>> {
        let (status, _) = identity_for(Some("")).await?;
        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
        Ok(())
    }
}
