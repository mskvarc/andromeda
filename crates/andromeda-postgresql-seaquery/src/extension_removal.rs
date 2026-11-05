//! Dropping an installed extension, refused while anything outside the
//! extension depends on its objects.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_iden,
};
use sea_query::{DynIden, IntoIden};

/// `DROP EXTENSION <extension> RESTRICT`.
///
/// `RESTRICT` refuses the drop with SQLSTATE `2BP01` while an object that is
/// not a member of the extension depends on one of its members, so the drop
/// never takes such an object with it.
#[derive(Clone, Debug)]
pub struct ExtensionRemoval(DynIden);

impl ExtensionRemoval {
    /// Drop `extension`.
    pub fn new(extension: impl IntoIden) -> ExtensionRemoval {
        ExtensionRemoval(extension.into_iden())
    }
}

impl RenderedCommand for ExtensionRemoval {
    fn rendered(&self) -> String {
        format!("DROP EXTENSION {} RESTRICT", quoted_iden(&self.0))
    }
}

impl Command for ExtensionRemoval {}

#[cfg(test)]
mod tests {
    use crate::{command::rendering::RenderedCommand, extension_removal::ExtensionRemoval};
    use sea_query::Alias;

    #[test]
    fn the_extension_is_named_as_a_quoted_identifier_and_dropped_only_without_dependents() {
        pretty_assertions::assert_eq!(
            ExtensionRemoval::new(Alias::new("pg_repack")).rendered(),
            r#"DROP EXTENSION "pg_repack" RESTRICT"#
        );
    }
}
