use crate::{
    api::router::create_router,
    core::{config::RuntimeConfig, error::AndromedaError},
};
use axum::{ServiceExt, middleware};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::normalize_path::NormalizePathLayer;
use tower_layer::Layer;
use tracing::info;

pub async fn run(config: RuntimeConfig) -> Result<(), AndromedaError> {
    let config = Arc::new(config);
    let router = create_router(Arc::clone(&config))
        .await?
        .layer(middleware::from_fn(crate::api::tenant::echo_tenant_header_middleware));

    // NormalizePathLayer must be outermost — applied before routing so that
    // trailing slashes are stripped before axum matches routes.
    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    let listener = TcpListener::bind(std::net::SocketAddr::from((config.server.host, config.server.port))).await?;

    info!("Listening on {}", listener.local_addr()?);

    axum::serve(listener, ServiceExt::<axum::extract::Request>::into_make_service(app)).await?;

    Ok(())
}
