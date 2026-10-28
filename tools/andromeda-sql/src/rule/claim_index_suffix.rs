//! `claim-index-suffix`: an index serving a queue claim is named `_claim`, so
//! every claim index reads the same.

use crate::rule::{Rule, RuleContext, RuleFinding};

/// The start of a word naming a claim.
const CLAIM_WORD: &str = "claim";

/// The ending of a claim index's name.
const CLAIM_SUFFIX: &str = "_claim";

/// A claim index not named `_claim`.
#[derive(Debug, derive_more::Display)]
#[display("{_0} serves a claim and does not end in _claim")]
struct ClaimNamed<'schema>(&'schema str);

/// Every index naming a claim without ending in `_claim`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .indexes()
        .iter()
        .filter(|index| index.name().split('_').any(|word| word.starts_with(CLAIM_WORD)) && !index.name().ends_with(CLAIM_SUFFIX))
        .map(|index| context.finding(Rule::ClaimIndexSuffix, index.origin(), index.name().len(), &ClaimNamed(index.name())))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_deliveries.sql";

    #[test]
    fn a_claim_index_is_named_claim() -> Result<(), FixtureError> {
        let sql = "create table deliveries (\n    due_at timestamptz\n);\n\ncreate index idx_deliveries_claim on deliveries (due_at);\n\ncreate index idx_deliveries_claimable on deliveries (due_at);\n";
        assert_eq!(
            findings(Rule::ClaimIndexSuffix, &[(FILE, sql)])?,
            ["migrations/branch/0001_deliveries.sql:7:14"]
        );
        Ok(())
    }
}
