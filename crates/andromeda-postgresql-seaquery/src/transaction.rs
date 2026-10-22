//! Beginning a transaction in the mode its statements need.
//!
//! The isolation level and the transaction-scoped settings travel with the
//! `BEGIN` in one round trip, so the transaction never runs a statement
//! before its mode is in force, and the connection leaves it with the
//! settings it had.

use crate::{
    command::rendering::RenderedCommand,
    setting::{Setting, SettingAssignment, SettingScope},
};
use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool, Postgres, Transaction};
use std::iter;

/// The isolation level a transaction runs at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Isolation {
    /// Each statement reads the data committed before it began: the server
    /// default.
    ReadCommitted,
    /// Every statement reads one snapshot, taken at the first one.
    RepeatableRead,
}

/// How a transaction begins: its isolation level and the settings in force
/// for it alone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionStart {
    /// The isolation level.
    isolation: Isolation,
    /// The `SET LOCAL` assignments made before its first statement.
    settings: Vec<SettingAssignment>,
}

impl TransactionStart {
    /// A transaction at `isolation`, with no setting of its own.
    #[must_use]
    pub const fn at(isolation: Isolation) -> TransactionStart {
        TransactionStart {
            isolation,
            settings: Vec::new(),
        }
    }

    /// The same start, with `setting` at `value` for the transaction alone.
    #[must_use]
    pub fn with_local(mut self, setting: Setting, value: &str) -> TransactionStart {
        self.settings.push(SettingAssignment::new(setting, value, SettingScope::Transaction));
        self
    }

    /// Begin the transaction on `connection`.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the transaction cannot begin or a
    /// setting cannot be applied.
    pub async fn begin_on<'connection>(&self, connection: &'connection mut PgConnection) -> Result<Transaction<'connection, Postgres>, sqlx::Error> {
        connection.begin_with(AssertSqlSafe(self.rendered())).await
    }

    /// Begin the transaction on a connection of `pool`.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when no connection is handed out in time,
    /// the transaction cannot begin or a setting cannot be applied.
    pub async fn begin_in(&self, pool: &PgPool) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
        pool.begin_with(AssertSqlSafe(self.rendered())).await
    }
}

impl RenderedCommand for TransactionStart {
    fn rendered(&self) -> String {
        let begin = match self.isolation {
            Isolation::ReadCommitted => "BEGIN".to_owned(),
            Isolation::RepeatableRead => "BEGIN ISOLATION LEVEL REPEATABLE READ".to_owned(),
        };
        iter::once(begin)
            .chain(self.settings.iter().map(SettingAssignment::rendered))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        setting::Setting,
        transaction::{Isolation, TransactionStart},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_repeatable_read_transaction_names_its_isolation_in_the_begin() {
        assert_eq!(
            TransactionStart::at(Isolation::RepeatableRead).rendered(),
            "BEGIN ISOLATION LEVEL REPEATABLE READ"
        );
    }

    #[test]
    fn transaction_settings_follow_the_begin_in_one_command() {
        assert_eq!(
            TransactionStart::at(Isolation::ReadCommitted)
                .with_local(Setting::PlanCacheMode, "force_custom_plan")
                .rendered(),
            "BEGIN; SET LOCAL plan_cache_mode = 'force_custom_plan'"
        );
    }
}
