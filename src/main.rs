mod api;
mod core;
mod health;
mod http;
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

use clap::Parser;
use core::{
    cli::{Cli, Command, ConfigCommand, OutputFormat},
    config::{RuntimeConfig, ServerOverrides},
    error::AndromedaError,
};
use jsonld::core_context::NGSI_LD_CORE_CONTEXT_V18_URL;
use mimalloc::MiMalloc;
use tracing::info;
use tracing_subscriber::fmt;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<(), AndromedaError> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run(run_args) => {
            let config = RuntimeConfig::load(
                &run_args.common.config_file,
                ServerOverrides {
                    host: run_args.host,
                    port: run_args.port,
                },
            )?;

            let subscriber = fmt().compact().with_max_level(config.observability.log_level.as_tracing_level()).finish();
            tracing::subscriber::set_global_default(subscriber)?;

            // Set the default core context URL to v1.8 (ETSI test suite expectation)
            jsonld::set_default_core_context_url(NGSI_LD_CORE_CONTEXT_V18_URL);
            info!("JSON-LD core context URL: {}", jsonld::default_core_context_url());
            info!(
                host = %config.server.host,
                port = config.server.port,
                config_file = %run_args.common.config_file.display(),
                cli_host_override = run_args.host.is_some(),
                cli_port_override = run_args.port.is_some(),
                tenancy_enabled = config.tenancy.enabled,
                outbox_enabled = config.outbox.enabled,
                log_level = %config.observability.log_level,
                precedence = RuntimeConfig::precedence_summary(),
                "Starting Andromeda"
            );

            server::run(config).await?;
        }
        Command::Config(config_args) => match config_args.command {
            ConfigCommand::Validate => {
                RuntimeConfig::load(&config_args.common.config_file, ServerOverrides::default())?;
                println!("Configuration valid: {}", config_args.common.config_file.display());
            }
            ConfigCommand::PrintEffective(print_args) => {
                let config = RuntimeConfig::load(&config_args.common.config_file, ServerOverrides::default())?;
                let rendered = match print_args.format {
                    OutputFormat::Json => config.to_redacted_json()?,
                    OutputFormat::Toml => config.to_redacted_toml()?,
                };
                println!("{rendered}");
            }
        },
    }

    Ok(())
}
