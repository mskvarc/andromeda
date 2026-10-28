//! `write-identity-suffix`: a unique constraint over a write's identity is
//! named `_write_identity`, the name the code recognises a replayed write by.

use crate::{
    declared_schema::constraint::ConstraintKind,
    rule::{Rule, RuleContext, RuleFinding},
};

/// The column a write's identity is held in.
const WRITE_IDENTITY_COLUMN: &str = "write_id";

/// The ending of a write identity constraint's name.
const WRITE_IDENTITY_SUFFIX: &str = "_write_identity";

/// A write identity constraint named otherwise.
#[derive(Debug, derive_more::Display)]
#[display("{_0} is unique over write_id and does not end in _write_identity")]
struct WriteIdentityNamed<'schema>(&'schema str);

/// Every unique constraint over `write_id` not named `_write_identity`.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .constraints()
        .iter()
        .filter(|constraint| constraint.kind() == ConstraintKind::Unique && constraint.columns().iter().any(|column| column == WRITE_IDENTITY_COLUMN))
        .filter_map(|constraint| Some((constraint.name()?, constraint.origin())))
        .filter(|(name, _)| !name.ends_with(WRITE_IDENTITY_SUFFIX))
        .map(|(name, origin)| context.finding(Rule::WriteIdentitySuffix, origin, name.len(), &WriteIdentityNamed(name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_history.sql";

    #[test]
    fn a_write_identity_constraint_is_named_write_identity() -> Result<(), FixtureError> {
        let sql = "create table history (\n    write_id uuid not null,\n    write_ordinal integer not null,\n    constraint uq_history_write_identity unique (write_id, write_ordinal),\n    constraint uq_history_write unique (write_id)\n);\n";
        assert_eq!(findings(Rule::WriteIdentitySuffix, &[(FILE, sql)])?, ["migrations/branch/0001_history.sql:5:5"]);
        Ok(())
    }
}
