//! Typed, state-complete health route inventory.

use crate::{handler, state::HealthRouteState};
use andromeda_health::aggregate::Health;
use axum::{Router, routing::get};

/// The complete health route fragment.
pub struct HealthRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl HealthRoutes {
    /// Construct `GET /health` with focused health state.
    pub fn new(health: Health) -> HealthRoutes {
        let router = Router::new().route("/health", get(handler::check)).with_state(HealthRouteState::new(health));
        HealthRoutes { router }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::routes::HealthRoutes;
    use andromeda_health::aggregate::Health;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::error::Error;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_fragment_owns_the_health_path() -> Result<(), Box<dyn Error>> {
        let response = HealthRoutes::new(Health::builder().build())
            .into_router()
            .oneshot(Request::get("/health").body(Body::empty())?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }
}
