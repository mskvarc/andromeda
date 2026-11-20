//! `function-attributes`: every function states its volatility and its
//! parallel safety.
//!
//! PostgreSQL takes a function that states neither as `volatile` and
//! `parallel unsafe`. That is right for a function with side effects and wrong
//! for one that only reads its arguments: a volatile function is never inlined
//! or folded into an index condition, and an unsafe one keeps the whole plan
//! serial. Stated on every function, each choice is read where the function is
//! written instead of being a default the reader has to remember. A trigger
//! function is exempt, since only its triggers call it and no planner ever
//! reads it in a query, and so is a procedure, which takes neither attribute.

use crate::{
    declared_schema::function::DeclaredFunction,
    rule::{Rule, RuleContext, RuleFinding},
};

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum AttributeViolation<'schema> {
    /// The function states no volatility.
    #[display("{_0} states no volatility, so it is volatile and never inlined")]
    Volatility(&'schema str),
    /// The function states no parallel safety.
    #[display("{_0} states no parallel safety, so it keeps every plan calling it serial")]
    ParallelSafety(&'schema str),
}

/// Every function, but a trigger function or a procedure, that leaves its
/// volatility or its parallel safety to the default.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .functions()
        .iter()
        .filter(|function| function.returns().is_some() && !function.is_trigger())
        .flat_map(|function| {
            let name = function.name().name();
            let missing = [
                function.volatility().is_none().then_some(AttributeViolation::Volatility(name)),
                function.parallel().is_none().then_some(AttributeViolation::ParallelSafety(name)),
            ];
            missing
                .into_iter()
                .flatten()
                .map(|violation| finding(context, function, &violation))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The finding of `violation` at the name of `function`.
fn finding(context: &RuleContext<'_>, function: &DeclaredFunction, violation: &AttributeViolation<'_>) -> RuleFinding {
    context.finding(Rule::FunctionAttributes, function.origin(), function.name().name().len(), violation)
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
    fn a_function_stating_both_attributes_is_accepted() -> Result<(), FixtureError> {
        let sql = "create function one ()\n    returns integer\n    language sql\n    immutable\n    parallel safe\n    return 1;\n";
        assert_eq!(findings(Rule::FunctionAttributes, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_trigger_function_and_a_procedure_are_exempt() -> Result<(), FixtureError> {
        let sql = "create function refuse ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    return null;\nend\n$$;\n\ncreate procedure noop ()\n    language sql\n    begin atomic\n        select 1;\n    end;\n";
        assert_eq!(findings(Rule::FunctionAttributes, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_function_leaving_either_attribute_to_the_default_is_refused_at_its_name() -> Result<(), FixtureError> {
        let sql = "create function one ()\n    returns integer\n    language sql\n    stable\n    return 1;\n\ncreate function two ()\n    returns integer\n    language sql\n    parallel safe\n    return 2;\n";
        assert_eq!(
            findings(Rule::FunctionAttributes, &[(FILE, sql)])?,
            ["migrations/tenant/0001_functions.sql:1:17", "migrations/tenant/0001_functions.sql:7:17"]
        );
        Ok(())
    }
}
