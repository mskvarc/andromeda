//! `bound-suffix`: a check that bounds a size is named `_length`, the word
//! every size check uses, never `_bound`.

use crate::rule::{Rule, RuleContext, RuleFinding};

/// The ending a size check may not have.
const BOUND_SUFFIX: &str = "_bound";

/// A check named `_bound`.
#[derive(Debug, derive_more::Display)]
#[display("{_0} is named _bound, where a size check is named _length")]
struct BoundNamed<'schema>(&'schema str);

/// Every constraint whose name ends in `_bound`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .constraints()
        .iter()
        .filter_map(|constraint| Some((constraint.name()?, constraint.origin())))
        .filter(|(name, _)| name.ends_with(BOUND_SUFFIX))
        .map(|(name, origin)| context.finding(Rule::BoundSuffix, origin, name.len(), &BoundNamed(name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_entities.sql";

    #[test]
    fn a_length_check_is_accepted_and_a_bound_check_refused() -> Result<(), FixtureError> {
        let sql = "create table entities (\n    id text,\n    constraint chk_entities_id_length check (octet_length(id) <= 1024),\n    constraint chk_entities_id_bound check (octet_length(id) > 0)\n);\n";
        assert_eq!(findings(Rule::BoundSuffix, &[(FILE, sql)])?, ["migrations/branch/0001_entities.sql:4:5"]);
        Ok(())
    }
}
