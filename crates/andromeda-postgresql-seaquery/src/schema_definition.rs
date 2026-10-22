//! Creating, renaming and dropping a PostgreSQL schema.
//!
//! sea-query models relations, not schemas, so these commands are spelled
//! here, the schema named as a quoted identifier.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_iden,
};
use sea_query::{DynIden, IntoIden};

/// One schema definition command.
#[derive(Clone, Debug)]
pub enum SchemaDefinition {
    /// `CREATE SCHEMA <schema>`.
    Create(DynIden),
    /// `ALTER SCHEMA <from> RENAME TO <to>`.
    Rename {
        /// The schema renamed.
        from: DynIden,
        /// Its new name.
        to: DynIden,
    },
    /// `DROP SCHEMA IF EXISTS <schema> CASCADE`: the schema and everything it
    /// holds; nothing when it does not exist.
    DropCascade(DynIden),
}

impl SchemaDefinition {
    /// `CREATE SCHEMA <schema>`.
    pub fn create(schema: impl IntoIden) -> SchemaDefinition {
        SchemaDefinition::Create(schema.into_iden())
    }

    /// `ALTER SCHEMA <from> RENAME TO <to>`.
    pub fn rename(from: impl IntoIden, to: impl IntoIden) -> SchemaDefinition {
        SchemaDefinition::Rename {
            from: from.into_iden(),
            to: to.into_iden(),
        }
    }

    /// `DROP SCHEMA IF EXISTS <schema> CASCADE`.
    pub fn drop_cascade(schema: impl IntoIden) -> SchemaDefinition {
        SchemaDefinition::DropCascade(schema.into_iden())
    }
}

impl RenderedCommand for SchemaDefinition {
    fn rendered(&self) -> String {
        match self {
            SchemaDefinition::Create(schema) => format!("CREATE SCHEMA {}", quoted_iden(schema)),
            SchemaDefinition::Rename { from, to } => format!("ALTER SCHEMA {} RENAME TO {}", quoted_iden(from), quoted_iden(to)),
            SchemaDefinition::DropCascade(schema) => format!("DROP SCHEMA IF EXISTS {} CASCADE", quoted_iden(schema)),
        }
    }
}

impl Command for SchemaDefinition {}

#[cfg(test)]
mod tests {
    use crate::{command::rendering::RenderedCommand, schema_definition::SchemaDefinition};
    use pretty_assertions::assert_eq;
    use sea_query::Alias;

    #[test]
    fn each_schema_command_quotes_the_schema_it_names() {
        assert_eq!(SchemaDefinition::create(Alias::new("snap_7_stg")).rendered(), r#"CREATE SCHEMA "snap_7_stg""#);
        assert_eq!(
            SchemaDefinition::rename(Alias::new("snap_7_stg"), Alias::new("snap_7")).rendered(),
            r#"ALTER SCHEMA "snap_7_stg" RENAME TO "snap_7""#
        );
        assert_eq!(
            SchemaDefinition::drop_cascade(Alias::new("snap_7")).rendered(),
            r#"DROP SCHEMA IF EXISTS "snap_7" CASCADE"#
        );
    }
}
