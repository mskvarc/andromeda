use crate::core::config::Configuration;
use clap::Parser;
use confique::Config;
use shadow_rs::shadow;
use std::path::PathBuf;

shadow!(build);

#[derive(Parser)]
#[command(version = build::CLAP_LONG_VERSION, about, long_about = None)]
#[command(disable_help_subcommand = true)]
#[clap(styles = clap_themes::CLAP_V3)]
pub struct Cli {
    #[arg(short = 'c', long = "config", default_value = "config/andromeda.toml")]
    pub config_file: PathBuf,

    #[command(flatten)]
    pub config: <Configuration as Config>::Layer,
}
