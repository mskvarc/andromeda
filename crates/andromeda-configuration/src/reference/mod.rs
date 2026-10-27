//! What the configuration tells about itself: the JSON Schema of the file,
//! the commented template `config generate` writes, and the descriptor a
//! documentation tool renders.
//!
//! All three read the operator description every table, key and choice
//! carries in `#[schemars(description = "...")]`, never its rustdoc, which is
//! written for the code. The template and the descriptor list keys in the
//! order the configuration serialises them, each at its built-in default, and
//! a key that is not set by default at the value the example configuration
//! gives it.

pub mod descriptor;
mod example;
pub mod json_schema;
mod key_listing;
mod schema_node;
pub mod template;

#[cfg(test)]
mod checked_in_tests;
#[cfg(test)]
mod tests;

use serde_json::Error as JsonError;
use thiserror::Error;
use toml::{de::Error as TomlReadError, ser::Error as TomlWriteError};
use toml_edit::TomlError;

/// The reference could not be generated.
#[derive(Debug, Error)]
pub enum ReferenceError {
    /// The schema could not be written as JSON, or read back.
    #[error("the configuration schema could not be written: {0}")]
    Schema(#[from] JsonError),
    /// An example value did not read as its key's type.
    #[error("an example value of the configuration does not read: {0}")]
    Example(#[from] TomlReadError),
    /// A configuration could not be written as TOML.
    #[error("the configuration could not be written as TOML: {0}")]
    Write(#[from] TomlWriteError),
    /// The written configuration did not read back as a TOML document.
    #[error("the written configuration is not a TOML document: {0}")]
    Document(#[from] TomlError),
}
