//! Creating and dropping a database, and its session defaults.
//!
//! None of these commands can take a bound parameter, and `CREATE DATABASE`
//! and `DROP DATABASE` cannot run inside a transaction, so they are spelled
//! here: the database named as a quoted identifier, a default's value as a
//! quoted literal.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_iden,
    literal::quoted_text,
    setting::Setting,
};
use sea_query::{DynIden, IntoIden};

/// What dropping a database does while sessions are connected to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectedSessions {
    /// The drop is refused while any session is connected, which keeps a
    /// database some broker still serves from being dropped underneath it.
    Refuse,
    /// The connected sessions are terminated first (`WITH (FORCE)`).
    Terminate,
}

/// One database definition command.
#[derive(Clone, Debug)]
pub enum DatabaseDefinition {
    /// `CREATE DATABASE <database>`.
    Create(DynIden),
    /// `DROP DATABASE IF EXISTS <database> [WITH (FORCE)]`.
    Drop {
        /// The database dropped.
        database: DynIden,
        /// Whether connected sessions are terminated or refuse the drop.
        sessions: ConnectedSessions,
    },
    /// `ALTER DATABASE <database> SET <setting> = '<value>'`: a default every
    /// future session of the database inherits, which survives a
    /// transaction-pooling proxy that binds a different backend to each
    /// transaction.
    SetDefault {
        /// The database configured.
        database: DynIden,
        /// The setting it defaults.
        setting: Setting,
        /// The default value.
        value: String,
    },
    /// `ALTER DATABASE <database> RESET ALL`: every default the database
    /// carries removed.
    ResetAll(DynIden),
}

impl DatabaseDefinition {
    /// `CREATE DATABASE <database>`.
    pub fn create(database: impl IntoIden) -> DatabaseDefinition {
        DatabaseDefinition::Create(database.into_iden())
    }

    /// `DROP DATABASE IF EXISTS <database>`, `sessions` deciding what happens
    /// to the ones still connected.
    pub fn drop(database: impl IntoIden, sessions: ConnectedSessions) -> DatabaseDefinition {
        DatabaseDefinition::Drop {
            database: database.into_iden(),
            sessions,
        }
    }

    /// `ALTER DATABASE <database> RESET ALL`.
    pub fn reset_all(database: impl IntoIden) -> DatabaseDefinition {
        DatabaseDefinition::ResetAll(database.into_iden())
    }

    /// `ALTER DATABASE <database> SET <setting> = '<value>'`.
    pub fn set_default(database: impl IntoIden, setting: Setting, value: impl Into<String>) -> DatabaseDefinition {
        DatabaseDefinition::SetDefault {
            database: database.into_iden(),
            setting,
            value: value.into(),
        }
    }
}

impl RenderedCommand for DatabaseDefinition {
    fn rendered(&self) -> String {
        match self {
            DatabaseDefinition::Create(database) => format!("CREATE DATABASE {}", quoted_iden(database)),
            DatabaseDefinition::Drop { database, sessions } => {
                let force = match sessions {
                    ConnectedSessions::Refuse => "",
                    ConnectedSessions::Terminate => " WITH (FORCE)",
                };
                format!("DROP DATABASE IF EXISTS {}{force}", quoted_iden(database))
            }
            DatabaseDefinition::SetDefault { database, setting, value } => {
                format!("ALTER DATABASE {} SET {setting} = {}", quoted_iden(database), quoted_text(value))
            }
            DatabaseDefinition::ResetAll(database) => format!("ALTER DATABASE {} RESET ALL", quoted_iden(database)),
        }
    }
}

impl Command for DatabaseDefinition {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        database_definition::{ConnectedSessions, DatabaseDefinition},
        setting::Setting,
    };
    use pretty_assertions::assert_eq;
    use sea_query::Alias;

    #[test]
    fn a_database_is_named_as_a_quoted_identifier() {
        assert_eq!(
            DatabaseDefinition::create(Alias::new("andromeda_tenant_a")).rendered(),
            r#"CREATE DATABASE "andromeda_tenant_a""#
        );
        assert_eq!(
            DatabaseDefinition::create(Alias::new("evil\"; DROP")).rendered(),
            r#"CREATE DATABASE "evil""; DROP""#
        );
    }

    #[test]
    fn a_drop_terminates_connected_sessions_only_when_asked() {
        assert_eq!(
            DatabaseDefinition::drop(Alias::new("t"), ConnectedSessions::Refuse).rendered(),
            r#"DROP DATABASE IF EXISTS "t""#
        );
        assert_eq!(
            DatabaseDefinition::drop(Alias::new("t"), ConnectedSessions::Terminate).rendered(),
            r#"DROP DATABASE IF EXISTS "t" WITH (FORCE)"#
        );
    }

    #[test]
    fn a_reset_removes_every_default_of_the_database() {
        assert_eq!(DatabaseDefinition::reset_all(Alias::new("t")).rendered(), r#"ALTER DATABASE "t" RESET ALL"#);
    }

    #[test]
    fn a_default_names_its_setting_and_quotes_its_value() {
        assert_eq!(
            DatabaseDefinition::set_default(Alias::new("t"), Setting::StatementTimeout, "35000ms").rendered(),
            r#"ALTER DATABASE "t" SET statement_timeout = '35000ms'"#
        );
    }
}
