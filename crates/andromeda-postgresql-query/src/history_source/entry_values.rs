//! The history columns an entry of a temporal append intent is recorded in.
//!
//! An entry holds the columns its history row will hold, so recording it and
//! reading it while it is still queued take the same values from it: the
//! application inserts them, the live history sources project them, and the
//! one mapping here keeps the two from disagreeing.

use andromeda_postgresql_schema::{
    temporal::{AttributeHistory, ScopeHistory},
    temporal_intent::TemporalEntry,
};
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType, postgis::st_make_valid};
use sea_query::SimpleExpr;

/// How an entry field's value is recorded in its column.
#[derive(Clone, Copy)]
enum Recorded {
    /// As the entry holds it.
    AsHeld,
    /// A geometry, made valid.
    MadeValid,
    /// A label path, cast to the `ltree` a `scopeQ` matches.
    AsLtree,
}

impl Recorded {
    /// `value` as it is recorded.
    fn of(self, value: SimpleExpr) -> SimpleExpr {
        match self {
            Recorded::AsHeld => value,
            Recorded::MadeValid => st_make_valid(value),
            Recorded::AsLtree => cast(value, PostgresType::Ltree),
        }
    }
}

/// How an entry's geometry is recorded in `value_geometry`.
const GEOMETRY_RECORDED: Recorded = Recorded::MadeValid;

/// The `attribute_history` value columns, each beside the entry field it is
/// recorded from and how, in the table's column order.
const ATTRIBUTE_FIELDS: [(AttributeHistory, TemporalEntry, Recorded); 22] = [
    (AttributeHistory::AttributeKind, TemporalEntry::AttributeKind, Recorded::AsHeld),
    (AttributeHistory::ValueKind, TemporalEntry::ValueKind, Recorded::AsHeld),
    (AttributeHistory::ObservedAt, TemporalEntry::ObservedAt, Recorded::AsHeld),
    (AttributeHistory::CreatedAt, TemporalEntry::CreatedAt, Recorded::AsHeld),
    (AttributeHistory::ModifiedAt, TemporalEntry::ModifiedAt, Recorded::AsHeld),
    (AttributeHistory::DeletedAt, TemporalEntry::DeletedAt, Recorded::AsHeld),
    (AttributeHistory::ValueDateTime, TemporalEntry::ValueDateTime, Recorded::AsHeld),
    (AttributeHistory::ExpiresAt, TemporalEntry::ExpiresAt, Recorded::AsHeld),
    (AttributeHistory::GeneratedInstanceId, TemporalEntry::GeneratedInstanceId, Recorded::AsHeld),
    (AttributeHistory::ValueTypeKey, TemporalEntry::ValueTypeKey, Recorded::AsHeld),
    (AttributeHistory::UnitCodeId, TemporalEntry::UnitCodeId, Recorded::AsHeld),
    (AttributeHistory::ValueBoolean, TemporalEntry::ValueBoolean, Recorded::AsHeld),
    (AttributeHistory::VerbatimInstanceId, TemporalEntry::VerbatimInstanceId, Recorded::AsHeld),
    (AttributeHistory::ValueNumber, TemporalEntry::ValueNumber, Recorded::AsHeld),
    (AttributeHistory::ValueString, TemporalEntry::ValueString, Recorded::AsHeld),
    (AttributeHistory::ValueGeometry, TemporalEntry::ValueGeometry, GEOMETRY_RECORDED),
    (AttributeHistory::ValueGeometryMembers, TemporalEntry::ValueGeometryMembers, Recorded::AsHeld),
    (AttributeHistory::ValueObject, TemporalEntry::ValueObject, Recorded::AsHeld),
    (AttributeHistory::ValueUri, TemporalEntry::ValueUri, Recorded::AsHeld),
    (AttributeHistory::ObjectTypeKeys, TemporalEntry::ObjectTypeKeys, Recorded::AsHeld),
    (AttributeHistory::ProofSeals, TemporalEntry::ProofSeals, Recorded::AsHeld),
    (AttributeHistory::SubAttributes, TemporalEntry::SubAttributes, Recorded::AsHeld),
];

/// The `scope_history` value columns, each beside the entry field it is
/// recorded from and how, in the table's column order.
const SCOPE_FIELDS: [(ScopeHistory, TemporalEntry, Recorded); 7] = [
    (ScopeHistory::ValueKind, TemporalEntry::ValueKind, Recorded::AsHeld),
    (ScopeHistory::ObservedAt, TemporalEntry::ObservedAt, Recorded::AsHeld),
    (ScopeHistory::CreatedAt, TemporalEntry::CreatedAt, Recorded::AsHeld),
    (ScopeHistory::ModifiedAt, TemporalEntry::ModifiedAt, Recorded::AsHeld),
    (ScopeHistory::DeletedAt, TemporalEntry::DeletedAt, Recorded::AsHeld),
    (ScopeHistory::ScopePath, TemporalEntry::ScopePath, Recorded::AsLtree),
    (ScopeHistory::VerbatimScopePath, TemporalEntry::VerbatimScopePath, Recorded::AsHeld),
];

/// The `attribute_history` value columns, in the table's column order, beside
/// the identity (`recorded_at`, `write_id`, `write_ordinal`) and the stream.
#[must_use]
pub fn attribute_value_columns() -> [AttributeHistory; 22] {
    ATTRIBUTE_FIELDS.map(|(column, ..)| column)
}

/// The `scope_history` value columns, in the table's column order, beside the
/// identity and the temporal entity.
#[must_use]
pub fn scope_value_columns() -> [ScopeHistory; 7] {
    SCOPE_FIELDS.map(|(column, ..)| column)
}

/// Every `attribute_history` value column beside the value an attribute entry
/// records in it, `field` reading one field of the entry.
///
/// The geometry is made valid, which is the one value the entry does not hold
/// as it is recorded.
pub fn attribute_entry_values(field: impl Fn(TemporalEntry) -> SimpleExpr) -> [(AttributeHistory, SimpleExpr); 22] {
    ATTRIBUTE_FIELDS.map(|(column, entry, recorded)| (column, recorded.of(field(entry))))
}

/// The geometry an attribute entry records in `value_geometry`, `field`
/// reading one field of the entry: `NULL` for an entry holding no geometry.
pub fn recorded_geometry(field: impl Fn(TemporalEntry) -> SimpleExpr) -> SimpleExpr {
    GEOMETRY_RECORDED.of(field(TemporalEntry::ValueGeometry))
}

/// Every `scope_history` value column beside the value a Scope entry records
/// in it, `field` reading one field of the entry.
///
/// The label path is cast to the `ltree` a `scopeQ` matches.
pub fn scope_entry_values(field: impl Fn(TemporalEntry) -> SimpleExpr) -> [(ScopeHistory, SimpleExpr); 7] {
    SCOPE_FIELDS.map(|(column, entry, recorded)| (column, recorded.of(field(entry))))
}

#[cfg(test)]
mod tests {
    use crate::history_source::entry_values::{attribute_entry_values, recorded_geometry, scope_entry_values};
    use andromeda_postgresql_schema::temporal_intent::TemporalEntry;
    use sea_query::{Alias, Expr, Iden, PostgresQueryBuilder, Query};

    fn rendered(values: impl IntoIterator<Item = (String, sea_query::SimpleExpr)>) -> Vec<String> {
        values
            .into_iter()
            .map(|(column, value)| format!("{column} <- {}", Query::select().expr(value).to_owned().to_string(PostgresQueryBuilder)))
            .collect()
    }

    #[test]
    fn an_attribute_entry_records_each_field_in_its_column_and_the_geometry_made_valid() {
        let values =
            rendered(attribute_entry_values(|field: TemporalEntry| Expr::col((Alias::new("e"), field))).map(|(column, value)| (column.to_string(), value)));
        assert!(values.contains(&r#"value_number <- SELECT "e"."value_number""#.to_owned()), "{values:?}");
        assert!(
            values.contains(&r#"value_geometry <- SELECT ST_MakeValid("e"."value_geometry")"#.to_owned()),
            "{values:?}"
        );
        assert!(values.contains(&r#"sub_attributes <- SELECT "e"."sub_attributes""#.to_owned()), "{values:?}");
        pretty_assertions::assert_eq!(values.len(), 22);
    }

    #[test]
    fn the_recorded_geometry_is_the_one_the_history_row_stores() {
        let entry = |field: TemporalEntry| Expr::col((Alias::new("e"), field));
        let values = rendered(attribute_entry_values(entry).map(|(column, value)| (column.to_string(), value)));
        let [geometry] = rendered([("value_geometry".to_owned(), recorded_geometry(entry))])
            .try_into()
            .expect("one rendering");
        assert!(values.contains(&geometry), "{geometry} in {values:?}");
    }

    /// Clause 4.5.6 gives the temporal Scope no `instanceId`, so a Scope row
    /// records no instance identity.
    #[test]
    fn a_scope_entry_records_its_label_path_as_an_ltree_and_no_instance_identity() {
        let values =
            rendered(scope_entry_values(|field: TemporalEntry| Expr::col((Alias::new("e"), field))).map(|(column, value)| (column.to_string(), value)));
        assert!(values.contains(&r#"scope_path <- SELECT "e"."scope_path"::ltree"#.to_owned()), "{values:?}");
        assert!(
            values.contains(&r#"verbatim_scope_path <- SELECT "e"."verbatim_scope_path""#.to_owned()),
            "{values:?}"
        );
        assert!(!values.iter().any(|value| value.contains("instance_id")), "{values:?}");
        pretty_assertions::assert_eq!(values.len(), 7);
    }
}
