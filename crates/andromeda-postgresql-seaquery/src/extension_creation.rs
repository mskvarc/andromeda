//! Creating an extension in the `public` schema.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_iden,
};
use sea_query::{DynIden, IntoIden};

/// `CREATE EXTENSION IF NOT EXISTS <extension> WITH SCHEMA public`.
///
/// The schema is named rather than left to the session's search path, so the
/// extension's objects land where every tenant database keeps them.
#[derive(Clone, Debug)]
pub struct ExtensionCreation(DynIden);

impl ExtensionCreation {
    /// Create `extension` unless the database already holds it.
    pub fn new(extension: impl IntoIden) -> ExtensionCreation {
        ExtensionCreation(extension.into_iden())
    }
}

impl RenderedCommand for ExtensionCreation {
    fn rendered(&self) -> String {
        format!("CREATE EXTENSION IF NOT EXISTS {} WITH SCHEMA \"public\"", quoted_iden(&self.0))
    }
}

impl Command for ExtensionCreation {}

#[cfg(test)]
mod tests {
    use crate::{command::rendering::RenderedCommand, extension_creation::ExtensionCreation};
    use sea_query::Alias;

    #[test]
    fn the_extension_is_named_as_a_quoted_identifier_and_created_in_public() {
        pretty_assertions::assert_eq!(
            ExtensionCreation::new(Alias::new("pgstattuple")).rendered(),
            r#"CREATE EXTENSION IF NOT EXISTS "pgstattuple" WITH SCHEMA "public""#
        );
    }
}
