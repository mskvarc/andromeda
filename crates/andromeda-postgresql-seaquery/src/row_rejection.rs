//! A row trigger that rejects the rows a condition selects, for a test that
//! needs a write to fail at a chosen row.
//!
//! The trigger runs a `plpgsql` function that raises an exception when the
//! condition holds for the row being deleted or updated, so the statement that
//! touches the row fails and its transaction rolls back. The function is named
//! after the trigger.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::{quoted_iden, quoted_table},
    literal::{int_literal, quoted_text},
};
use sea_query::{Alias, DynIden, IntoIden, PostgresQueryBuilder, Query, SimpleExpr, TableName};
use strum::Display;

/// The row event a rejecting trigger fires before.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum RejectedEvent {
    /// `BEFORE DELETE`: the condition reads the row as `old`.
    #[strum(serialize = "DELETE")]
    Delete,
    /// `BEFORE UPDATE`: the condition reads the row as `new`.
    #[strum(serialize = "UPDATE")]
    Update,
}

impl RejectedEvent {
    /// The row variable the trigger function returns for the event.
    const fn returned_row(self) -> &'static str {
        match self {
            RejectedEvent::Delete => "OLD",
            RejectedEvent::Update => "NEW",
        }
    }
}

/// A trigger that raises an exception for every row of `table` that satisfies
/// `condition`.
///
/// The condition reads the row through the lower-case record names `old` and
/// `new`, quoted like any other relation: `Expr::col((Alias::new("old"),
/// column))`.
#[derive(Clone, Debug)]
pub struct RejectingTrigger {
    /// The trigger's name; the function it runs is `<name>_injection`.
    name: DynIden,
    /// The relation the trigger belongs to.
    table: TableName,
    /// The event the trigger fires before.
    event: RejectedEvent,
    /// The predicate over the row that makes the trigger raise.
    condition: SimpleExpr,
    /// The exception message.
    message: String,
}

impl RejectingTrigger {
    /// A trigger named `name` on `table` that raises `message` for the rows of
    /// `event` that satisfy `condition`.
    pub fn new(name: impl IntoIden, table: impl Into<TableName>, event: RejectedEvent, condition: SimpleExpr, message: impl Into<String>) -> RejectingTrigger {
        RejectingTrigger {
            name: name.into_iden(),
            table: table.into(),
            event,
            condition,
            message: message.into(),
        }
    }

    /// The quoted name of the trigger function.
    fn function(&self) -> String {
        quoted_iden(&Alias::new(format!("{}_injection", self.name)).into_iden())
    }

    /// The command that installs the function and the trigger.
    #[must_use]
    pub const fn install(&self) -> RejectingTriggerCommand<'_> {
        RejectingTriggerCommand::Install(self)
    }

    /// The command that removes the trigger and its function.
    #[must_use]
    pub const fn remove(&self) -> RejectingTriggerCommand<'_> {
        RejectingTriggerCommand::Remove(self)
    }
}

/// Installing or removing a [`RejectingTrigger`].
#[derive(Clone, Copy, Debug)]
pub enum RejectingTriggerCommand<'trigger> {
    /// `CREATE OR REPLACE FUNCTION` and `CREATE TRIGGER`.
    Install(&'trigger RejectingTrigger),
    /// `DROP TRIGGER` and `DROP FUNCTION`.
    Remove(&'trigger RejectingTrigger),
}

impl RenderedCommand for RejectingTriggerCommand<'_> {
    fn rendered(&self) -> String {
        match self {
            RejectingTriggerCommand::Install(trigger) => {
                let probe = Query::select()
                    .expr(int_literal(1))
                    .and_where(trigger.condition.clone())
                    .to_string(PostgresQueryBuilder);
                format!(
                    "CREATE OR REPLACE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF EXISTS ({probe}) THEN RAISE EXCEPTION {message}; END IF; RETURN {row}; END $$; \
                     CREATE TRIGGER {name} BEFORE {event} ON {table} FOR EACH ROW EXECUTE FUNCTION {function}()",
                    function = trigger.function(),
                    message = quoted_text(&trigger.message),
                    row = trigger.event.returned_row(),
                    name = quoted_iden(&trigger.name),
                    event = trigger.event,
                    table = quoted_table(&trigger.table),
                )
            }
            RejectingTriggerCommand::Remove(trigger) => format!(
                "DROP TRIGGER {name} ON {table}; DROP FUNCTION {function}()",
                name = quoted_iden(&trigger.name),
                table = quoted_table(&trigger.table),
                function = trigger.function(),
            ),
        }
    }
}

impl Command for RejectingTriggerCommand<'_> {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        row_rejection::{RejectedEvent, RejectingTrigger},
    };
    use sea_query::{Alias, Expr, ExprTrait};

    fn purge_fault() -> RejectingTrigger {
        RejectingTrigger::new(
            Alias::new("purge_failure"),
            Alias::new("snapshots"),
            RejectedEvent::Delete,
            Expr::col((Alias::new("old"), Alias::new("id"))).like("%:fault"),
            "deterministic purge fault",
        )
    }

    #[test]
    fn the_install_command_creates_the_function_and_the_trigger_on_the_event() {
        pretty_assertions::assert_eq!(
            purge_fault().install().rendered(),
            "CREATE OR REPLACE FUNCTION \"purge_failure_injection\"() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF EXISTS (SELECT 1 WHERE \"old\".\"id\" LIKE '%:fault') THEN RAISE EXCEPTION 'deterministic purge fault'; END IF; RETURN OLD; END $$; \
             CREATE TRIGGER \"purge_failure\" BEFORE DELETE ON \"snapshots\" FOR EACH ROW EXECUTE FUNCTION \"purge_failure_injection\"()"
        );
    }

    #[test]
    fn the_remove_command_drops_the_trigger_before_its_function() {
        pretty_assertions::assert_eq!(
            purge_fault().remove().rendered(),
            "DROP TRIGGER \"purge_failure\" ON \"snapshots\"; DROP FUNCTION \"purge_failure_injection\"()"
        );
    }
}
