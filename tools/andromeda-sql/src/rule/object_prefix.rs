//! `object-prefix`: a constraint, index or statistics name starts with the
//! prefix of its kind followed by its own table's name or registered
//! abbreviation, so the name alone says what it is and where it lives.

use crate::{
    declared_schema::{constraint::ConstraintKind, origin::Origin},
    rule::{Rule, RuleContext, RuleFinding, naming_vocabulary::has_table_prefix},
};

/// An object whose name does not start with its kind and table.
#[derive(Debug, derive_more::Display)]
#[display("{name} does not start with {prefix}_ and the name or abbreviation of {table}")]
struct MisprefixedObject<'schema> {
    /// The name.
    name: &'schema str,
    /// The prefix of its kind.
    prefix: &'static str,
    /// Its table.
    table: &'schema str,
}

/// The prefix a constraint of `kind` is named with.
const fn constraint_prefix(kind: ConstraintKind) -> &'static str {
    match kind {
        ConstraintKind::PrimaryKey => "pk",
        ConstraintKind::Unique => "uq",
        ConstraintKind::ForeignKey => "fk",
        ConstraintKind::Check => "chk",
        ConstraintKind::Exclusion => "ex",
    }
}

/// Every constraint, index and statistics object named without its kind's
/// prefix and its table, an unnamed constraint included.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let constraints = schema.constraints().iter().map(|constraint| {
        (
            constraint.name().unwrap_or_default(),
            constraint_prefix(constraint.kind()),
            constraint.table(),
            constraint.origin(),
        )
    });
    let indexes = schema
        .indexes()
        .iter()
        .map(|index| (index.name(), if index.is_unique() { "uq" } else { "idx" }, index.table(), index.origin()));
    let statistics = schema
        .statistics()
        .iter()
        .map(|statistics| (statistics.name(), "stx", statistics.table(), statistics.origin()));
    constraints
        .chain(indexes)
        .chain(statistics)
        .filter(|(name, prefix, table, _)| !has_table_prefix(name, prefix, table))
        .map(|(name, prefix, table, origin): (&str, &'static str, &str, Origin)| {
            context.finding(Rule::ObjectPrefix, origin, name.len(), &MisprefixedObject { name, prefix, table })
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

    const FILE: &str = "tenant/0001_tenants.sql";

    #[test]
    fn prefixed_names_are_accepted() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    tenant_key bigint not null,\n    constraint pk_tenants primary key (tenant_key)\n);\n\ncreate index idx_tenants_tenant_key on tenants (tenant_key);\n";
        assert_eq!(findings(Rule::ObjectPrefix, &[(FILE, sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_misprefixed_constraint_and_index_are_refused_at_their_names() -> Result<(), FixtureError> {
        let sql = "create table tenants (\n    tenant_key bigint not null,\n    constraint tenants_pkey primary key (tenant_key)\n);\n\ncreate index idx_tenant_key on tenants (tenant_key);\n";
        assert_eq!(
            findings(Rule::ObjectPrefix, &[(FILE, sql)])?,
            ["migrations/tenant/0001_tenants.sql:3:5", "migrations/tenant/0001_tenants.sql:6:14"]
        );
        Ok(())
    }
}
