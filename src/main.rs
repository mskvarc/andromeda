mod api;
mod core;
mod domain;
mod health;
mod http;
mod i18n;
mod jsonld;
mod query;
mod renderer;
mod repository;
mod server;
mod service;
mod tenancy;
mod transport;
mod version;
mod worker;

use core::{config::Configuration, error::AndromedaError};
use mimalloc::MiMalloc;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<(), AndromedaError> {
    let config = Configuration::new()?;

    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).finish();

    tracing::subscriber::set_global_default(subscriber)?;

    // Set the default core context URL to v1.8 (ETSI test suite expectation)
    crate::jsonld::set_default_core_context_url(crate::jsonld::core_context::NGSI_LD_CORE_CONTEXT_V18_URL);
    info!("JSON-LD core context URL: {}", crate::jsonld::default_core_context_url());

    info!("Starting Andromeda...");

    info!("Configuration: {:#?}", config);

    server::run(config).await?;

    Ok(())
}
