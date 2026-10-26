//! The commented default configuration `configuration generate` writes.

use crate::broker_config::BrokerConfig;
use toml_comment::TomlComment;

/// The default configuration as TOML, every setting preceded by the
/// documentation of the field it comes from and shown at its built-in default.
#[must_use]
pub fn default_template() -> String {
    <BrokerConfig as TomlComment>::default_toml()
}
