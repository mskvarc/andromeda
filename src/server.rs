use crate::{api::router::create_router, core::config::Configuration, error::AndromedaError};
use axum::{ServiceExt, middleware};
use tokio::net::TcpListener;
use tower_http::normalize_path::NormalizePathLayer;
use tower_layer::Layer;
use tracing::info;

pub async fn run(config: &Configuration) -> Result<(), AndromedaError> {
    let router = create_router(config)
        .await?
        .layer(middleware::from_fn(crate::api::tenant::echo_tenant_header_middleware));

    // NormalizePathLayer must be outermost — applied before routing so that
    // trailing slashes are stripped before axum matches routes.
    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    let listener = TcpListener::bind(&format!("{}:{}", config.andromeda.host, config.andromeda.port)).await?;

    info!("Listening on {}", listener.local_addr()?);

    axum::serve(listener, ServiceExt::<axum::extract::Request>::into_make_service(app)).await?;

    Ok(())
}
