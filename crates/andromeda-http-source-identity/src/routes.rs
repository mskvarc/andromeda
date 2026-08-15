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
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::error::Error;
    use tower::ServiceExt;

    #[tokio::test]
    async fn source_identity_fragment_owns_its_complete_path() -> Result<(), Box<dyn Error>> {
        let state = SourceIdentityState::new(
            "urn:ngsi-ld:ContextSourceIdentity:test".parse::<Uri>()?,
            "test".parse::<ContextSourcePseudonym>()?,
        );
        let response = SourceIdentityRoutes::new(state)
            .into_router()
            .oneshot(Request::get("/info/sourceIdentity").body(Body::empty())?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }
}
