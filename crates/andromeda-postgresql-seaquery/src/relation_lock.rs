//! Locking relations until the transaction ends, optionally without waiting.
//!
//! `LOCK TABLE` has no sea-query form. A destructive statement over many
//! relations takes every lock it needs first and without waiting, so a
//! transaction still open on any of them fails it at once while it holds
//! nothing, instead of leaving it queued behind that transaction holding the
//! locks it already took. A fixture that needs a relation held against other
//! sessions takes the same lock and waits for it.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_tables,
};
use sea_query::TableName;
use strum::Display;

/// A table lock mode, spelled as `LOCK TABLE ... IN <mode> MODE` spells it.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum LockMode {
    /// `ACCESS EXCLUSIVE`: conflicts with every other lock, reads included.
    #[strum(serialize = "ACCESS EXCLUSIVE")]
    AccessExclusive,
    /// `ROW EXCLUSIVE`: the lock a data-changing statement takes.
    #[strum(serialize = "ROW EXCLUSIVE")]
    RowExclusive,
    /// `SHARE`: conflicts with every data-changing lock.
    #[strum(serialize = "SHARE")]
    Share,
}

/// Whether the command waits for a conflicting lock to clear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockWait {
    /// Queue behind the conflicting lock.
    Wait,
    /// Fail with `lock_not_available` (55P03) when some relation is in use.
    NoWait,
}

/// `LOCK TABLE <relations> IN <mode> MODE [NOWAIT]`: every relation locked
/// until the transaction ends.
#[derive(Clone, Debug)]
pub struct RelationLock {
    /// The relations locked, in one command.
    relations: Vec<TableName>,
    /// The mode every relation is locked in.
    mode: LockMode,
    /// Whether the command waits for a conflicting lock.
    wait: LockWait,
}

impl RelationLock {
    /// Lock `relations` exclusively without waiting.
    #[must_use]
    pub const fn exclusive_nowait(relations: Vec<TableName>) -> RelationLock {
        RelationLock {
            relations,
            mode: LockMode::AccessExclusive,
            wait: LockWait::NoWait,
        }
    }

    /// Lock `relations` in `mode`, queueing behind any conflicting lock.
    #[must_use]
    pub const fn waiting(relations: Vec<TableName>, mode: LockMode) -> RelationLock {
        RelationLock {
            relations,
            mode,
            wait: LockWait::Wait,
        }
    }
}

impl RenderedCommand for RelationLock {
    fn rendered(&self) -> String {
        let wait = match self.wait {
            LockWait::Wait => "",
            LockWait::NoWait => " NOWAIT",
        };
        format!("LOCK TABLE {} IN {} MODE{wait}", quoted_tables(&self.relations), self.mode)
    }
}

impl Command for RelationLock {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        relation_lock::{LockMode, RelationLock},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, TableName};

    #[test]
    fn every_relation_is_locked_in_one_command_without_waiting() {
        let relations = vec![
            TableName::from((Alias::new("public"), Alias::new("entities"))),
            TableName::from((Alias::new("snap_7"), Alias::new("entity_search"))),
        ];
        assert_eq!(
            RelationLock::exclusive_nowait(relations).rendered(),
            r#"LOCK TABLE "public"."entities", "snap_7"."entity_search" IN ACCESS EXCLUSIVE MODE NOWAIT"#
        );
    }

    #[test]
    fn a_waiting_lock_names_its_mode_and_omits_the_wait_clause() {
        let relations = vec![
            TableName::from(Alias::new("notification_events")),
            TableName::from(Alias::new("notification_deliveries")),
        ];
        assert_eq!(
            RelationLock::waiting(relations, LockMode::Share).rendered(),
            r#"LOCK TABLE "notification_events", "notification_deliveries" IN SHARE MODE"#
        );
        assert_eq!(
            RelationLock::waiting(vec![TableName::from(Alias::new("entities"))], LockMode::RowExclusive).rendered(),
            r#"LOCK TABLE "entities" IN ROW EXCLUSIVE MODE"#
        );
    }
}
