//! Typed, state-complete active-component route inventory.

use crate::{handler, state::ComponentReportState};
use andromeda_component_report::ComponentReport;
use axum::{Router, routing::get};

/// The complete active-component report route fragment.
pub struct ComponentReportRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl ComponentReportRoutes {
    /// Construct `GET /version` from the active component inventory.
    pub fn new(components: Vec<ComponentReport>) -> ComponentReportRoutes {
        let router = Router::new()
            .route("/version", get(handler::get))
            .with_state(ComponentReportState::new(components));
        ComponentReportRoutes { router }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::routes::ComponentReportRoutes;
    use andromeda_component_report::{ComponentReport, ComponentVersion};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::error::Error;
    use tower::ServiceExt;

    #[tokio::test]
    async fn component_report_fragment_owns_the_version_path() -> Result<(), Box<dyn Error>> {
        let response = ComponentReportRoutes::new(vec![ComponentReport::PostgreSql {
            version: ComponentVersion::from("16.2".to_owned()),
        }])
        .into_router()
        .oneshot(Request::get("/version").body(Body::empty())?)
        .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }
}
