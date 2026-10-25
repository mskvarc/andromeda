//! The commands of the `PgBouncer` admin console.
//!
//! The console's pseudo-database answers `SHOW` and control commands over the
//! simple query protocol only, never a prepared statement, so each one is a
//! [`Command`]. A database is named as the console parses it: a bare word.

use crate::command::{Command, rendering::RenderedCommand};
use thiserror::Error;

/// A database name the console accepts: letters, digits and underscores.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolerDatabase(String);

/// Why a name is not one the console accepts.
#[derive(Debug, Error)]
#[error("a pooler console database name holds only ASCII letters, digits and underscores")]
pub struct UnacceptedPoolerDatabase;

impl PoolerDatabase {
    /// The database named `name`.
    ///
    /// # Errors
    ///
    /// Returns [`UnacceptedPoolerDatabase`] when `name` is empty or holds a
    /// character other than an ASCII letter, a digit or an underscore.
    pub fn new(name: &str) -> Result<PoolerDatabase, UnacceptedPoolerDatabase> {
        if !name.is_empty() && name.chars().all(|character| character.is_ascii_alphanumeric() || character == '_') {
            Ok(PoolerDatabase(name.to_owned()))
        } else {
            Err(UnacceptedPoolerDatabase)
        }
    }
}

/// One admin console command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PoolerCommand {
    /// `PAUSE`: new client transactions wait until a resume.
    Pause,
    /// `RESUME`: every paused pool resumes.
    Resume,
    /// `RESUME <database>`: one paused or killed database resumes.
    ResumeDatabase(PoolerDatabase),
    /// `RECONNECT`: every server connection reconnects at its next
    /// opportunity.
    Reconnect,
    /// `KILL <database>`: every client and server connection of one database
    /// is dropped at once.
    Kill(PoolerDatabase),
    /// `SHOW POOLS`: one row per (database, user) pool.
    ShowPools,
    /// `SHOW CLIENTS`: one row per client connection.
    ShowClients,
}

impl RenderedCommand for PoolerCommand {
    fn rendered(&self) -> String {
        match self {
            PoolerCommand::Pause => "PAUSE".to_owned(),
            PoolerCommand::Resume => "RESUME".to_owned(),
            PoolerCommand::ResumeDatabase(database) => format!("RESUME {}", database.0),
            PoolerCommand::Reconnect => "RECONNECT".to_owned(),
            PoolerCommand::Kill(database) => format!("KILL {}", database.0),
            PoolerCommand::ShowPools => "SHOW POOLS".to_owned(),
            PoolerCommand::ShowClients => "SHOW CLIENTS".to_owned(),
        }
    }
}

impl Command for PoolerCommand {}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        pooler_console::{PoolerCommand, PoolerDatabase},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_database_command_names_the_database_as_a_bare_word() {
        let database = PoolerDatabase::new("andromeda_tenant_a").expect("an accepted name");
        assert_eq!(PoolerCommand::Kill(database).rendered(), "KILL andromeda_tenant_a");
    }

    #[test]
    fn a_name_the_console_would_read_as_more_than_a_word_is_refused() {
        assert!(PoolerDatabase::new("a; SHUTDOWN").is_err());
        assert!(PoolerDatabase::new("").is_err());
    }
}
