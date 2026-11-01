//! The words the naming rules read: the registered table abbreviations, the
//! words no name may carry, and the specification members a column keeps the
//! name of.

/// The abbreviation each table's constraint, index, statistics and trigger
/// names use in place of its full name, as `(table, abbreviation)`. A table
/// listed here is named by its abbreviation only.
pub const TABLE_ABBREVIATIONS: &[(&str, &str)] = &[
    ("current_attributes", "ca"),
    ("context_source_registrations", "csr"),
    ("pending_side_effects", "pse"),
];

/// Abbreviations no name may use at all.
const UNREGISTERED_ABBREVIATIONS: &[&str] = &["attr", "db"];

/// Storage-type words no name may carry: a name says what a value means.
const STORAGE_TYPE_WORDS: &[&str] = &["uuid", "geo", "bool", "datetime"];

/// The columns that hold a specification member under the member's own name,
/// which the word and boolean rules do not apply to, as `(table, column)`.
pub const SPECIFICATION_MEMBER_COLUMNS: &[(&str, &str)] = &[
    ("context_source_registrations", "management_local_only"),
    ("subscriptions", "geo_q"),
    ("subscriptions", "local_only"),
    ("subscriptions", "split_entities"),
];

/// Where a name is used, which decides whether a registered abbreviation is
/// allowed in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    /// A table or a column, which spells every word out.
    Relation,
    /// A constraint, index, statistics object or trigger, which uses its
    /// table's registered abbreviation.
    Object,
}

/// Whether `table.column` is a specification member kept under its own name.
#[must_use]
pub fn is_specification_member(table: &str, column: &str) -> bool {
    SPECIFICATION_MEMBER_COLUMNS.contains(&(table, column))
}

/// The first word of `name` it may not carry, if any.
#[must_use]
pub fn forbidden_word(name: &str, kind: NameKind) -> Option<&str> {
    name.split('_').find(|word| {
        UNREGISTERED_ABBREVIATIONS.contains(word)
            || STORAGE_TYPE_WORDS.contains(word)
            || (kind == NameKind::Relation && TABLE_ABBREVIATIONS.iter().any(|(_, abbreviation)| abbreviation == word))
    })
}

/// The part of an object name that names `table`: its registered
/// abbreviation when it has one, otherwise its full name.
#[must_use]
pub fn table_part(table: &str) -> &str {
    TABLE_ABBREVIATIONS
        .iter()
        .find(|(abbreviated, _)| *abbreviated == table)
        .map_or(table, |(_, abbreviation)| *abbreviation)
}

/// Whether `name` is `<prefix>_<table part>`, optionally followed by `_` and
/// more, where the table part is the one [`table_part`] gives.
#[must_use]
pub fn has_table_prefix(name: &str, prefix: &str, table: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
        .and_then(|rest| rest.strip_prefix(table_part(table)))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('_'))
}

#[cfg(test)]
mod tests {
    use crate::rule::naming_vocabulary::{NameKind, forbidden_word, has_table_prefix, table_part};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_table_part_is_the_registered_abbreviation_or_else_the_full_table_name() {
        assert_eq!(table_part("current_attributes"), "ca");
        assert_eq!(table_part("context_source_registrations"), "csr");
        assert_eq!(table_part("pending_side_effects"), "pse");
        assert_eq!(table_part("tenants"), "tenants");
    }

    #[test]
    fn an_abbreviated_table_is_named_only_by_its_abbreviation() {
        assert!(has_table_prefix("pk_csr", "pk", "context_source_registrations"));
        assert!(has_table_prefix("idx_ca_value_number", "idx", "current_attributes"));
        assert!(!has_table_prefix("pk_context_source_registrations", "pk", "context_source_registrations"));
        assert!(!has_table_prefix(
            "chk_context_source_registrations_id_length",
            "chk",
            "context_source_registrations"
        ));
        assert!(!has_table_prefix("chk_current_attributes_x", "chk", "current_attributes"));
    }

    #[test]
    fn a_table_without_an_abbreviation_is_named_by_its_full_name() {
        assert!(has_table_prefix("pk_tenants", "pk", "tenants"));
        assert!(has_table_prefix("idx_tenants_status", "idx", "tenants"));
        assert!(!has_table_prefix("chk_tenant_status", "chk", "tenants"));
        assert!(!has_table_prefix("chk_pse_status", "chk", "tenants"));
        assert!(!has_table_prefix("idx_tenants_x", "uq", "tenants"));
        assert!(!has_table_prefix("idx_tenantsx", "idx", "tenants"));
    }

    #[test]
    fn abbreviations_are_forbidden_in_table_and_column_names_only() {
        assert_eq!(forbidden_word("csr_delivery_state", NameKind::Relation), Some("csr"));
        assert_eq!(forbidden_word("chk_csr_mode", NameKind::Object), None);
        assert_eq!(forbidden_word("idx_ca_attr_number", NameKind::Object), Some("attr"));
        assert_eq!(forbidden_word("value_geo", NameKind::Relation), Some("geo"));
        assert_eq!(forbidden_word("value_geometry", NameKind::Relation), None);
    }
}
