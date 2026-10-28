//! `set-isolation`: a migration references only the relations its own set
//! can reach where it runs.
//!
//! The platform set runs on the platform database alone, so it references
//! only platform relations, and the tenant and branch sets, which run in every
//! tenant database, never reference one. A test against the default tenant,
//! whose database holds both, would pass while every named tenant failed. The
//! branch set also runs alone inside every Snapshot schema, so it references
//! only the relations a branch migration creates (and the types and functions
//! of `public`, which its search path reaches), and it creates no type, domain
//! or extension, which would live in that schema alone. A foreign key into a
//! tenant relation is such a reference too, and would let a Snapshot's
//! deletion wait on a lock in the live branch.

use crate::{
    declared_schema::created_object::ObjectKind,
    migration_set::MigrationSet,
    rule::{Rule, RuleContext, RuleFinding},
};
use std::collections::{BTreeMap, BTreeSet};

/// The schema a qualified reference may name and still mean a migration's
/// own relation.
const PUBLIC_SCHEMA: &str = "public";

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum IsolationViolation<'schema> {
    /// A platform migration names a tenant or branch relation.
    #[display("a platform migration references {_0}, which lives in every tenant database")]
    PlatformReachesTenant(&'schema str),
    /// A tenant or branch migration names a platform relation.
    #[display("a {set} migration references {relation}, which lives in the platform database")]
    TenantReachesPlatform {
        /// The set of the migration.
        set: MigrationSet,
        /// The relation.
        relation: &'schema str,
    },
    /// A branch migration names a relation no branch migration creates.
    #[display("a branch migration references {_0}, which no Snapshot schema holds")]
    BranchReachesTenant(&'schema str),
    /// A branch migration creates a type, a domain or an extension.
    #[display("a branch migration creates the {kind} {name}, which would live in one Snapshot schema alone")]
    BranchCreatesShared {
        /// What it creates.
        kind: ObjectKind,
        /// Its name.
        name: &'schema str,
    },
}

/// Every reference across sets, and every type, domain or extension a branch
/// migration creates.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let schema = context.schema();
    let mut creators: BTreeMap<&str, BTreeSet<MigrationSet>> = BTreeMap::new();
    for object in schema
        .created()
        .iter()
        .filter(|object| matches!(object.kind(), ObjectKind::Table | ObjectKind::View))
    {
        if let Some(set) = schema.set_of(object.origin()) {
            creators.entry(object.name()).or_default().insert(set);
        }
    }
    let references = schema.references().iter().filter_map(|reference| {
        let name = reference.name();
        if name.schema().is_some_and(|qualifier| qualifier != PUBLIC_SCHEMA) {
            return None;
        }
        let created_by = creators.get(name.name())?;
        let set = schema.set_of(reference.origin())?;
        let relation = name.name();
        let violation = match set {
            MigrationSet::Platform if !created_by.contains(&MigrationSet::Platform) => IsolationViolation::PlatformReachesTenant(relation),
            MigrationSet::Tenant | MigrationSet::Branch if created_by.iter().all(|creator| *creator == MigrationSet::Platform) => {
                IsolationViolation::TenantReachesPlatform { set, relation }
            }
            MigrationSet::Branch if !created_by.contains(&MigrationSet::Branch) => IsolationViolation::BranchReachesTenant(relation),
            MigrationSet::Platform | MigrationSet::Tenant | MigrationSet::Branch => return None,
        };
        Some(context.finding(Rule::SetIsolation, reference.origin(), relation.len(), &violation))
    });
    let created_in_branch = schema
        .created()
        .iter()
        .filter(|object| matches!(object.kind(), ObjectKind::CompositeType | ObjectKind::Domain | ObjectKind::Extension))
        .filter(|object| schema.set_of(object.origin()) == Some(MigrationSet::Branch))
        .map(|object| {
            context.finding(
                Rule::SetIsolation,
                object.origin(),
                object.name().len(),
                &IsolationViolation::BranchCreatesShared {
                    kind: object.kind(),
                    name: object.name(),
                },
            )
        });
    references.chain(created_in_branch).collect()
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const TENANTS: (&str, &str) = ("platform/0001_tenants.sql", "create table tenants (\n    tenant_key bigint not null\n);\n");

    const NAMES: (&str, &str) = (
        "tenant/0002_names.sql",
        "create table attribute_names (\n    attribute_name_key integer not null\n);\n",
    );

    #[test]
    fn each_set_referencing_its_own_relations_is_accepted() -> Result<(), FixtureError> {
        let entities = (
            "branch/0003_entities.sql",
            "create table entities (\n    entity_key bigint not null\n);\n\ncreate index idx_entities_entity_key on entities (entity_key);\n",
        );
        let link = (
            "tenant/0004_links.sql",
            "alter table attribute_names\n    add column entity_key bigint references entities (entity_key);\n",
        );
        assert_eq!(findings(Rule::SetIsolation, &[TENANTS, NAMES, entities, link])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_tenant_migration_referencing_a_platform_relation_is_refused() -> Result<(), FixtureError> {
        let index = ("tenant/0002_index.sql", "create index idx_tenants_tenant_key on tenants (tenant_key);\n");
        assert_eq!(findings(Rule::SetIsolation, &[TENANTS, index])?, ["migrations/tenant/0002_index.sql:1:40"]);
        Ok(())
    }

    #[test]
    fn a_branch_foreign_key_into_a_tenant_relation_is_refused() -> Result<(), FixtureError> {
        let history = (
            "branch/0003_history.sql",
            "create table history (\n    attribute_name_key integer references attribute_names (attribute_name_key)\n);\n",
        );
        assert_eq!(findings(Rule::SetIsolation, &[NAMES, history])?, ["migrations/branch/0003_history.sql:2:43"]);
        Ok(())
    }

    #[test]
    fn a_branch_migration_creating_a_type_is_refused() -> Result<(), FixtureError> {
        let types = ("branch/0003_types.sql", "create type pair as (\n    left_value integer\n);\n");
        assert_eq!(findings(Rule::SetIsolation, &[types])?, ["migrations/branch/0003_types.sql:1:13"]);
        Ok(())
    }
}
