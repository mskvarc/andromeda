//! The configured `contextSourceExtras` as a JSON literal.

use crate::{JsonLiteral, admission::JsonLiteralError};
use andromeda_configuration::schema::source_extras::{SourceExtras, SourceExtrasError};
use thiserror::Error;

/// Why the configured extras are not a JSON literal.
#[derive(Debug, Error)]
pub enum ConfiguredExtrasError {
    /// A value has no JSON spelling.
    #[error(transparent)]
    Unspellable(#[from] SourceExtrasError),
    /// The JSON text is not admissible as a literal.
    #[error("identity.source_extras: {0}")]
    Literal(#[from] JsonLiteralError),
}

// The configuration spells the table as JSON text; admission is this crate's.
impl TryFrom<&SourceExtras> for JsonLiteral {
    type Error = ConfiguredExtrasError;

    fn try_from(extras: &SourceExtras) -> Result<JsonLiteral, ConfiguredExtrasError> {
        Ok(JsonLiteral::admit(&extras.to_json_text()?)?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{JsonLiteral, configured_extras::ConfiguredExtrasError};
    use andromeda_configuration::schema::source_extras::{SourceExtras, SourceExtrasError};
    use pretty_assertions::assert_eq;

    #[test]
    fn configured_extras_become_the_literal_they_spell() {
        let extras: SourceExtras = toml::from_str("region = \"eu-west\"\nnodes = [1, 2]\n[nested]\nraw = true\n").expect("a table");
        assert_eq!(
            JsonLiteral::try_from(&extras).expect("a literal"),
            JsonLiteral::admit(r#"{"region": "eu-west", "nodes": [1, 2], "nested": {"raw": true}}"#).expect("a JSON literal")
        );
    }

    #[test]
    fn a_date_time_is_refused_before_admission() {
        let extras: SourceExtras = toml::from_str("at = 1979-05-27\n").expect("a table");
        assert!(matches!(
            JsonLiteral::try_from(&extras),
            Err(ConfiguredExtrasError::Unspellable(SourceExtrasError::DateTime))
        ));
    }
}
