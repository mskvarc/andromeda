//! `no-catch-all-exception`: no exception handler catches `others`.
//!
//! A block that catches `others` to skip one expected failure swallows every
//! other failure too, a real installation error included, and the migration
//! then succeeds having done nothing. A block probes for what it needs first
//! (`pg_available_extensions`, a catalogue row) or catches the one condition
//! it expects.

use crate::{
    declared_schema::origin::Origin,
    plpgsql_tree::PlpgsqlRoutine,
    rule::{Rule, RuleContext, RuleFinding},
};

/// The condition that catches every error.
const OTHERS: &str = "others";

/// The length of the `do` keyword a block's finding is placed on.
const DO_KEYWORD_LENGTH: usize = 2;

/// A handler catching every error.
#[derive(Debug, derive_more::Display)]
#[display("{_0} has an exception handler that catches others")]
struct CatchAll<'schema>(&'schema str);

/// Every function and `do` block with a handler catching `others`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let catches_all = |routines: &[PlpgsqlRoutine]| routines.iter().any(|routine| routine.caught_conditions().contains(&OTHERS));
    let functions = schema
        .functions()
        .iter()
        .filter(|function| catches_all(function.routines()))
        .map(|function| (function.origin(), function.name().name().len(), function.name().name()));
    let blocks = schema
        .do_blocks()
        .iter()
        .filter(|block| catches_all(block.routines()))
        .map(|block| (block.origin(), DO_KEYWORD_LENGTH, "a do block"));
    functions
        .chain(blocks)
        .map(|(origin, length, routine): (Origin, usize, &str)| context.finding(Rule::NoCatchAllException, origin, length, &CatchAll(routine)))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "tenant/0001_extensions.sql";

    #[test]
    fn a_block_catching_one_condition_is_accepted() -> Result<(), FixtureError> {
        let sql =
            "do $$\nbegin\n    create extension if not exists x;\nexception\n    when feature_not_supported then\n        raise notice 'skipped';\nend\n$$;\n";
        assert_eq!(findings(Rule::NoCatchAllException, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_block_catching_others_is_refused_at_do() -> Result<(), FixtureError> {
        let sql =
            "-- Skips x.\ndo $$\nbegin\n    create extension if not exists x;\nexception\n    when others then\n        raise notice 'skipped';\nend\n$$;\n";
        assert_eq!(
            findings(Rule::NoCatchAllException, &[(FILE, sql)])?,
            ["migrations/tenant/0001_extensions.sql:2:1"]
        );
        Ok(())
    }
}
