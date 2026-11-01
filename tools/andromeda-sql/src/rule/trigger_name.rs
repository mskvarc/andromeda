//! `trigger-name`: a trigger is named `trg_<table>_<what>` after the table it
//! fires on, spelt as its registered abbreviation when it has one and as its
//! full name otherwise.

use crate::rule::{Rule, RuleContext, RuleFinding, naming_vocabulary::table_part};

/// The start of a trigger's name.
const TRIGGER_PREFIX: &str = "trg_";

/// A trigger named otherwise.
#[derive(Debug, derive_more::Display)]
#[display("{trigger} fires on {table} and is not named trg_{part}_<what>")]
struct MisnamedTrigger<'schema> {
    /// The trigger.
    trigger: &'schema str,
    /// Its table.
    table: &'schema str,
    /// The part of the name that names its table.
    part: &'schema str,
}

/// Whether `name` is `trg_<table part>_<what>` with a non-empty `<what>`.
fn is_trigger_name(name: &str, table: &str) -> bool {
    name.strip_prefix(TRIGGER_PREFIX)
        .and_then(|rest| rest.strip_prefix(table_part(table)))
        .and_then(|rest| rest.strip_prefix('_'))
        .is_some_and(|what| !what.is_empty())
}

/// Every trigger not named after its table.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .triggers()
        .iter()
        .filter(|trigger| !is_trigger_name(trigger.name(), trigger.table()))
        .map(|trigger| {
            context.finding(
                Rule::TriggerName,
                trigger.origin(),
                trigger.name().len(),
                &MisnamedTrigger {
                    trigger: trigger.name(),
                    table: trigger.table(),
                    part: table_part(trigger.table()),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
        trigger_name::is_trigger_name,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_trigger_name_is_its_table_part_and_what_it_does() {
        assert!(is_trigger_name("trg_ca_entities_insert", "current_attributes"));
        assert!(is_trigger_name("trg_attribute_names_refuse", "attribute_names"));
        assert!(!is_trigger_name("trg_current_attributes_entities_insert", "current_attributes"));
        assert!(!is_trigger_name("trg_ai_entities_insert", "current_attributes"));
        assert!(!is_trigger_name("trg_ca", "current_attributes"));
        assert!(!is_trigger_name("trg_ca_", "current_attributes"));
        assert!(!is_trigger_name("ca_entities_insert", "current_attributes"));
    }

    #[test]
    fn a_trigger_named_after_another_table_is_refused_at_its_name() -> Result<(), FixtureError> {
        let sql = "create function refuse () returns trigger language plpgsql as $$\nbegin\n    return null;\nend\n$$;\n\ncreate trigger trg_names_refuse\n    before delete on attribute_names for each row\n    execute function refuse ();\n";
        assert_eq!(
            findings(Rule::TriggerName, &[("tenant/0001_names.sql", sql)])?,
            ["migrations/tenant/0001_names.sql:7:16"]
        );
        Ok(())
    }
}
