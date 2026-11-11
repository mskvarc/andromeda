//! `constraint-on-sealed-table`: a foreign key or check added to a table a
//! sealed migration created is added `not valid`, and a later migration
//! validates it, unless the table is a hypertable.
//!
//! A sealed table may hold rows on every installation, so adding the
//! constraint checks all of them under the `access exclusive` lock
//! `alter table` takes, blocking every read and write for the length of a
//! table scan. Added `not valid`, it holds that lock only for a moment and
//! checks new rows only; `validate constraint` then checks the existing rows
//! under a lock that lets reads and writes go on, in a migration of its own so
//! the two locks are never held in one transaction. A table created by a
//! migration not yet sealed is still empty when its release deploys, so a
//! constraint added to it costs nothing and the rule passes it over.
//!
//! TimescaleDB refuses `validate constraint` on a hypertable whose columnstore
//! is enabled, and the broker enables it at run time, so a constraint added
//! `not valid` to a hypertable could never be validated. A constraint on a
//! hypertable is therefore added validated, in the one statement that checks
//! the rows its uncompressed chunks hold.

use crate::{
    declared_schema::constraint::{ConstraintKind, DeclaredConstraint},
    rule::{Rule, RuleContext, RuleFinding},
};

/// A constraint added to a sealed table the costly way.
#[derive(Debug, derive_more::Display)]
enum UnstagedConstraint<'schema> {
    /// The constraint checks every existing row as it is added.
    #[display("{kind} {name} is added to {table}, which a sealed migration created, without not valid, so it checks every row under an access exclusive lock")]
    Validated {
        /// What the constraint enforces.
        kind: ConstraintKind,
        /// The constraint, or how it is written when unnamed.
        name: &'schema str,
        /// The table.
        table: &'schema str,
    },
    /// The constraint is added `not valid` to a hypertable, which can never
    /// validate it.
    #[display("{kind} {name} is added not valid to {table}, a hypertable, which cannot validate it once its columnstore is enabled")]
    Unvalidatable {
        /// What the constraint enforces.
        kind: ConstraintKind,
        /// The constraint.
        name: &'schema str,
        /// The table.
        table: &'schema str,
    },
    /// The constraint is added `not valid` and nothing later validates it.
    #[display("{kind} {name} is added not valid to {table}, which a sealed migration created, and no later migration validates it")]
    NeverValidated {
        /// What the constraint enforces.
        kind: ConstraintKind,
        /// The constraint.
        name: &'schema str,
        /// The table.
        table: &'schema str,
    },
}

/// How an unnamed constraint is named in a finding.
const UNNAMED: &str = "an unnamed constraint";

/// Every foreign key or check on a sealed table that is not added `not valid`
/// and validated by a later migration, or on a sealed hypertable that is not
/// added validated.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let ledger = context.ledger();
    schema
        .constraints()
        .iter()
        .filter(|constraint| matches!(constraint.kind(), ConstraintKind::ForeignKey | ConstraintKind::Check))
        .filter(|constraint| {
            schema
                .tables()
                .iter()
                .any(|table| table.name() == constraint.table() && ledger.seals_version(table.origin().version()))
        })
        .filter_map(|constraint| {
            let name = constraint.name().unwrap_or(UNNAMED);
            let hypertable = schema.hypertables().iter().any(|hypertable| hypertable.table() == constraint.table());
            let kind = constraint.kind();
            let table = constraint.table();
            let violation = match (constraint.name(), hypertable, constraint.is_not_valid()) {
                (_, true, false) => return None,
                (Some(name), false, true) if is_validated_later(context, constraint, name) => return None,
                (_, true, true) => UnstagedConstraint::Unvalidatable { kind, name, table },
                (Some(name), false, true) => UnstagedConstraint::NeverValidated { kind, name, table },
                (_, false, false) | (None, false, true) => UnstagedConstraint::Validated { kind, name, table },
            };
            Some(context.finding(
                Rule::ConstraintOnSealedTable,
                constraint.origin(),
                constraint.name().map_or(0, str::len),
                &violation,
            ))
        })
        .collect()
}

/// Whether a migration after the one adding `constraint` validates it by
/// `name`.
fn is_validated_later(context: &RuleContext<'_>, constraint: &DeclaredConstraint, name: &str) -> bool {
    context.schema().validations().iter().any(|validation| {
        validation.table() == constraint.table() && validation.name() == name && validation.origin().version() > constraint.origin().version()
    })
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings_sealing},
    };
    use pretty_assertions::assert_eq;

    const ENTITIES: (&str, &str) = (
        "tenant/0001_entities.sql",
        "create table entities (\n    entity_key bigint not null,\n    kind_key bigint not null\n);\n\ncreate table kinds (\n    kind_key bigint not null\n);\n",
    );

    const STAGED_CHECK: (&str, &str) = (
        "tenant/0002_entity_kind_check.sql",
        "alter table entities\n    add constraint chk_entities_kind check (kind_key >= 0) not valid;\n",
    );

    const VALIDATION: (&str, &str) = (
        "tenant/0003_entity_kind_validation.sql",
        "alter table entities\n    validate constraint chk_entities_kind;\n",
    );

    #[test]
    fn a_check_added_not_valid_and_validated_by_a_later_migration_is_accepted() -> Result<(), FixtureError> {
        assert_eq!(
            findings_sealing(Rule::ConstraintOnSealedTable, &[ENTITIES, STAGED_CHECK, VALIDATION], &[ENTITIES.0])?,
            Vec::<String>::new()
        );
        Ok(())
    }

    #[test]
    fn a_check_added_to_a_sealed_table_without_not_valid_is_refused_at_its_constraint() -> Result<(), FixtureError> {
        let check = (
            "tenant/0002_entity_kind_check.sql",
            "alter table entities\n    add constraint chk_entities_kind check (kind_key >= 0);\n",
        );
        assert_eq!(
            findings_sealing(Rule::ConstraintOnSealedTable, &[ENTITIES, check], &[ENTITIES.0])?,
            ["migrations/tenant/0002_entity_kind_check.sql:2:9"]
        );
        Ok(())
    }

    #[test]
    fn a_foreign_key_added_to_a_sealed_table_without_not_valid_is_refused() -> Result<(), FixtureError> {
        let reference = (
            "tenant/0002_entity_kind_reference.sql",
            "alter table entities\n    add constraint fk_entities_kind foreign key (kind_key) references kinds (kind_key);\n",
        );
        assert_eq!(
            findings_sealing(Rule::ConstraintOnSealedTable, &[ENTITIES, reference], &[ENTITIES.0])?,
            ["migrations/tenant/0002_entity_kind_reference.sql:2:9"]
        );
        Ok(())
    }

    #[test]
    fn a_check_added_not_valid_and_never_validated_is_refused() -> Result<(), FixtureError> {
        assert_eq!(
            findings_sealing(Rule::ConstraintOnSealedTable, &[ENTITIES, STAGED_CHECK], &[ENTITIES.0])?,
            ["migrations/tenant/0002_entity_kind_check.sql:2:9"]
        );
        Ok(())
    }

    #[test]
    fn a_validation_in_the_same_migration_is_refused() -> Result<(), FixtureError> {
        let both = (
            "tenant/0002_entity_kind_check.sql",
            "alter table entities\n    add constraint chk_entities_kind check (kind_key >= 0) not valid;\n\nalter table entities\n    validate constraint chk_entities_kind;\n",
        );
        assert_eq!(
            findings_sealing(Rule::ConstraintOnSealedTable, &[ENTITIES, both], &[ENTITIES.0])?,
            ["migrations/tenant/0002_entity_kind_check.sql:2:9"]
        );
        Ok(())
    }

    const HYPERTABLE: (&str, &str) = (
        "tenant/0002_entity_hypertable.sql",
        "select\n    create_hypertable('entities', by_range('entity_key'));\n",
    );

    #[test]
    fn a_check_added_validated_to_a_sealed_hypertable_is_accepted() -> Result<(), FixtureError> {
        let check = (
            "tenant/0003_entity_kind_check.sql",
            "alter table entities\n    add constraint chk_entities_kind check (kind_key >= 0);\n",
        );
        assert_eq!(
            findings_sealing(Rule::ConstraintOnSealedTable, &[ENTITIES, HYPERTABLE, check], &[ENTITIES.0, HYPERTABLE.0])?,
            Vec::<String>::new()
        );
        Ok(())
    }

    #[test]
    fn a_check_added_not_valid_to_a_sealed_hypertable_is_refused_though_validated_later() -> Result<(), FixtureError> {
        let check = (
            "tenant/0003_entity_kind_check.sql",
            "alter table entities\n    add constraint chk_entities_kind check (kind_key >= 0) not valid;\n",
        );
        let validation = (
            "tenant/0004_entity_kind_validation.sql",
            "alter table entities\n    validate constraint chk_entities_kind;\n",
        );
        assert_eq!(
            findings_sealing(
                Rule::ConstraintOnSealedTable,
                &[ENTITIES, HYPERTABLE, check, validation],
                &[ENTITIES.0, HYPERTABLE.0]
            )?,
            ["migrations/tenant/0003_entity_kind_check.sql:2:9"]
        );
        Ok(())
    }

    #[test]
    fn a_constraint_added_to_a_table_no_release_has_sealed_is_passed_over() -> Result<(), FixtureError> {
        let check = (
            "tenant/0002_entity_kind_check.sql",
            "alter table entities\n    add constraint chk_entities_kind check (kind_key >= 0);\n",
        );
        assert_eq!(findings_sealing(Rule::ConstraintOnSealedTable, &[ENTITIES, check], &[])?, Vec::<String>::new());
        Ok(())
    }
}
