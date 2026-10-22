//! Updating an installed extension to the version the server ships.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_iden,
};
use sea_query::{DynIden, IntoIden};

/// `ALTER EXTENSION <extension> UPDATE`.
///
/// `TimescaleDB` accepts it only as the first statement of a session, before
/// the old library is loaded.
#[derive(Clone, Debug)]
pub struct ExtensionUpdate(DynIden);

impl ExtensionUpdate {
    /// Update `extension`.
    pub fn new(extension: impl IntoIden) -> ExtensionUpdate {
        ExtensionUpdate(extension.into_iden())
    }
}

impl RenderedCommand for ExtensionUpdate {
    fn rendered(&self) -> String {
        format!("ALTER EXTENSION {} UPDATE", quoted_iden(&self.0))
    }
}

impl Command for ExtensionUpdate {}

#[cfg(test)]
mod tests {
    use crate::{command::rendering::RenderedCommand, extension_update::ExtensionUpdate};
    use sea_query::Alias;

    #[test]
    fn the_extension_is_named_as_a_quoted_identifier() {
        pretty_assertions::assert_eq!(
            ExtensionUpdate::new(Alias::new("timescaledb")).rendered(),
            r#"ALTER EXTENSION "timescaledb" UPDATE"#
        );
    }
}
