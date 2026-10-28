//! `no-security-definer`: no function runs with its owner's privileges.
//!
//! A `security definer` function lends its owner's rights to every caller, so
//! a function the broker's role can call becomes a way around the grants the
//! operator set, and a body that resolves names through the caller's search
//! path can be steered into running another object as the owner.

use crate::{
    declared_schema::token_sequence::token_runs,
    rule::{Rule, RuleContext, RuleFinding},
};
use pg_query::protobuf::Token;

/// The tokens that make a function run as its owner.
const SECURITY_DEFINER: [Token; 2] = [Token::Security, Token::Definer];

/// A `security definer`.
#[derive(Debug, derive_more::Display)]
#[display("security definer runs the function with its owner's privileges")]
struct SecurityDefiner;

/// Every `security definer`, in a `create function` or an `alter function`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .migrations()
        .iter()
        .flat_map(|migration| {
            token_runs(migration, &SECURITY_DEFINER)
                .into_iter()
                .map(|run| context.finding(Rule::NoSecurityDefiner, migration.origin(run.start), run.len(), &SecurityDefiner))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "tenant/0001_functions.sql";

    #[test]
    fn a_function_running_as_its_caller_is_accepted() -> Result<(), FixtureError> {
        let sql = "create function one ()\n    returns integer\n    language sql\n    security invoker\n    return 1;\n";
        assert_eq!(findings(Rule::NoSecurityDefiner, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_security_definer_function_is_refused_at_the_clause() -> Result<(), FixtureError> {
        let sql = "create function one ()\n    returns integer\n    language sql\n    security definer\n    return 1;\n";
        assert_eq!(findings(Rule::NoSecurityDefiner, &[(FILE, sql)])?, ["migrations/tenant/0001_functions.sql:4:5"]);
        Ok(())
    }
}
