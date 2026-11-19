//! The statements a test runs to leave an extension in a state the server's
//! own scripts do not: installed below the version the server ships, recorded
//! at a version no script installed, or with its members held by an object
//! outside it.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_iden,
    literal::quoted_text,
};
use sea_query::{DynIden, IntoIden};

/// `CREATE EXTENSION <extension> WITH SCHEMA public VERSION '<version>'`,
/// installing a version the server still ships below its default.
#[derive(Clone, Debug)]
pub struct ExtensionCreationAtVersion {
    /// The extension.
    extension: DynIden,
    /// The version to install, as `pg_available_extension_versions` names it.
    version: String,
}

impl ExtensionCreationAtVersion {
    /// Create `extension` at `version`.
    pub fn new(extension: impl IntoIden, version: &str) -> ExtensionCreationAtVersion {
        ExtensionCreationAtVersion {
            extension: extension.into_iden(),
            version: version.to_owned(),
        }
    }
}

impl RenderedCommand for ExtensionCreationAtVersion {
    fn rendered(&self) -> String {
        format!(
            "CREATE EXTENSION {} WITH SCHEMA \"public\" VERSION {}",
            quoted_iden(&self.extension),
            quoted_text(&self.version)
        )
    }
}

impl Command for ExtensionCreationAtVersion {}

/// `UPDATE pg_catalog.pg_extension SET extversion = '<version>' WHERE extname
/// = '<extension>'`, which records a version for an installed extension
/// without running any of its scripts: a version below the shipped one of an
/// extension that ships no update scripts, or text no version reads, as a
/// `PostGIS` upgrade leaves in place while it runs.
#[derive(Clone, Debug)]
pub struct InstalledVersionRecord {
    /// The extension, as `pg_extension` names it.
    extension: DynIden,
    /// The text recorded as its version.
    version: String,
}

impl InstalledVersionRecord {
    /// Record `version` for the installed `extension`.
    pub fn new(extension: impl IntoIden, version: &str) -> InstalledVersionRecord {
        InstalledVersionRecord {
            extension: extension.into_iden(),
            version: version.to_owned(),
        }
    }
}

impl RenderedCommand for InstalledVersionRecord {
    fn rendered(&self) -> String {
        format!(
            "UPDATE pg_catalog.pg_extension SET extversion = {} WHERE extname = {}",
            quoted_text(&self.version),
            quoted_text(&self.extension.to_string())
        )
    }
}

impl Command for InstalledVersionRecord {}

/// `CREATE VIEW public.<view> AS SELECT * FROM <schema>.<relation>`, an
/// object outside an extension that depends on one of its member relations,
/// so a `RESTRICT` drop of the extension is refused.
#[derive(Clone, Debug)]
pub struct MemberView {
    /// The view, created in `public`.
    view: DynIden,
    /// The schema of the member relation.
    schema: DynIden,
    /// The member relation.
    relation: DynIden,
}

impl MemberView {
    /// Create `view` over `schema.relation`.
    pub fn new(view: impl IntoIden, schema: impl IntoIden, relation: impl IntoIden) -> MemberView {
        MemberView {
            view: view.into_iden(),
            schema: schema.into_iden(),
            relation: relation.into_iden(),
        }
    }
}

impl RenderedCommand for MemberView {
    fn rendered(&self) -> String {
        format!(
            "CREATE VIEW \"public\".{} AS SELECT * FROM {}.{}",
            quoted_iden(&self.view),
            quoted_iden(&self.schema),
            quoted_iden(&self.relation)
        )
    }
}

impl Command for MemberView {}

/// `DROP VIEW public.<view>`, letting the extension a [`MemberView`] held go.
#[derive(Clone, Debug)]
pub struct MemberViewRemoval(DynIden);

impl MemberViewRemoval {
    /// Drop `view`.
    pub fn new(view: impl IntoIden) -> MemberViewRemoval {
        MemberViewRemoval(view.into_iden())
    }
}

impl RenderedCommand for MemberViewRemoval {
    fn rendered(&self) -> String {
        format!("DROP VIEW \"public\".{}", quoted_iden(&self.0))
    }
}

impl Command for MemberViewRemoval {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        test_extension::{ExtensionCreationAtVersion, InstalledVersionRecord, MemberView, MemberViewRemoval},
    };
    use pretty_assertions::assert_eq;
    use sea_query::Alias;

    #[test]
    fn each_statement_names_its_objects_as_quoted_identifiers_and_its_versions_as_literals() {
        assert_eq!(
            ExtensionCreationAtVersion::new(Alias::new("timescaledb"), "2.31.0").rendered(),
            r#"CREATE EXTENSION "timescaledb" WITH SCHEMA "public" VERSION '2.31.0'"#
        );
        assert_eq!(
            InstalledVersionRecord::new(Alias::new("pg_repack"), "1.5.2").rendered(),
            "UPDATE pg_catalog.pg_extension SET extversion = '1.5.2' WHERE extname = 'pg_repack'"
        );
        assert_eq!(
            MemberView::new(Alias::new("repack_watch"), Alias::new("repack"), Alias::new("tables")).rendered(),
            r#"CREATE VIEW "public"."repack_watch" AS SELECT * FROM "repack"."tables""#
        );
        assert_eq!(
            MemberViewRemoval::new(Alias::new("repack_watch")).rendered(),
            r#"DROP VIEW "public"."repack_watch""#
        );
    }
}
