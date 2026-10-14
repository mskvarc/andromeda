//! What an `attribute_history` or `scope_history` row that no longer reads back
//! as the observation it recorded looks like.
//!
//! The broker only ever writes rows it validated, so each variant is a column
//! holding something the broker never wrote, or a combination of columns its
//! check constraints rule out. A column that cannot be read at all is a
//! `sqlx` decode failure and is reported as one, naming the column itself.

use andromeda_context_commit::write_ordinal::WriteOrdinalOutOfRange;
use andromeda_ngsi_ld_entity::refusal::EntityRefusal;
use andromeda_ngsi_ld_lexicon::{identifier::IdentifierParseError, reference::NgsiLdReferenceError, scope::path::ScopePathError, uri::UriParseError};
use andromeda_ngsi_ld_model::temporal::structured_value_codec::StoredValueError;
use andromeda_ngsi_ld_number::JsonNumberError;
use andromeda_postgresql_attribute_instance::{assemble::AssemblyError, stored_members::StoredInstanceError};
use andromeda_postgresql_codes::{registry::UnknownCompactCode, stored_unit_code::UnknownStoredUnitCode};
use andromeda_postgresql_encoding::ewkb::EwkbError;
use strum::{Display, IntoStaticStr};
use thiserror::Error;

/// The history hypertable a row was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "snake_case")]
pub enum HistoryTable {
    /// `attribute_history`, one row per Attribute instance.
    AttributeHistory,
    /// `scope_history`, one row per Scope path or removal.
    ScopeHistory,
}

/// A history column whose value a row's declared shape requires.
///
/// Each renders as the name the history statements project it under, so the
/// same value names the column in a failure and looks it up in the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum HistoryColumn {
    /// The operational instant the row was written.
    RecordedAt,
    /// The instant a tombstone row records the deletion at.
    DeletedAt,
    /// The `observedAt` Temporal Property.
    ObservedAt,
    /// The `createdAt` Temporal Property.
    CreatedAt,
    /// The `modifiedAt` Temporal Property.
    ModifiedAt,
    /// The instant a scope event is keyed by on the requested axis.
    ScopeEventAt,
    /// The Scope a scope row holds, as it was written.
    VerbatimScopePath,
    /// The value of a String row.
    ValueString,
    /// The value of a Boolean row.
    ValueBoolean,
    /// The value of an Object row.
    ValueObject,
    /// The value of a `DateTime` row.
    ValueDateTime,
    /// The value of a URI row.
    ValueUri,
    /// The value of a Number row.
    ValueNumber,
    /// The value of a Geometry row, projected as EWKB.
    ValueGeometryEwkb,
}

/// The column a stored unit-code identity was read from.
///
/// Each renders as the name the statement projects it under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum UnitCodeColumn {
    /// The unit of one history row.
    UnitCodeId,
    /// The lowest unit an aggregation group's rows carried.
    UnitCodeIdLowest,
    /// The highest unit an aggregation group's rows carried.
    UnitCodeIdHighest,
}

/// A stored history row that does not read back as the observation it recorded.
#[derive(Debug, Error)]
pub enum StoredHistoryError {
    /// The attribute family code is not one the registry assigned.
    #[error("a stored attribute_history attribute_kind is not a known code: {0}")]
    AttributeKind(#[source] UnknownCompactCode),
    /// The value kind code is not one the registry assigned.
    #[error("a stored {table} value_kind is not a known code: {source}")]
    ValueKind {
        /// The table the row came from.
        table: HistoryTable,
        /// The registry's rejection.
        #[source]
        source: UnknownCompactCode,
    },
    /// The stored attribute IRI is not an NGSI-LD name.
    #[error("a stored attribute name is not an NGSI-LD name: {0}")]
    AttributeName(#[source] NgsiLdReferenceError),
    /// The stored `datasetId` is not a URI.
    #[error("a stored datasetId is not a URI: {0}")]
    DatasetId(#[source] IdentifierParseError),
    /// The verbatim `instanceId` is not a URI.
    #[error("a stored {table} verbatim_instance_id is not a URI: {source}")]
    VerbatimInstanceId {
        /// The table the row came from.
        table: HistoryTable,
        /// The URI grammar's rejection.
        #[source]
        source: IdentifierParseError,
    },
    /// Both instance identifier columns are populated, which the check
    /// constraint that makes them mutually exclusive rules out.
    #[error("a {table} row populates both instance identifier columns")]
    BothInstanceIdentifiers {
        /// The table the row came from.
        table: HistoryTable,
    },
    /// Neither instance identifier column is populated.
    #[error("a {table} row populates neither instance identifier column")]
    NoInstanceIdentifier {
        /// The table the row came from.
        table: HistoryTable,
    },
    /// The write ordinal is outside the range the write identity numbers.
    #[error("a stored {table} write_ordinal is out of range: {source}")]
    WriteOrdinal {
        /// The table the row came from.
        table: HistoryTable,
        /// The ordinal range's rejection.
        #[source]
        source: WriteOrdinalOutOfRange,
    },
    /// A column the row's declared shape requires holds no value.
    #[error("{table}.{column} is empty but the row declares a value there")]
    AbsentColumn {
        /// The table the row came from.
        table: HistoryTable,
        /// The column that holds no value.
        column: HistoryColumn,
    },
    /// A stored unit-code identity names no code this build lists.
    #[error("a stored {column} names no unit code: {source}")]
    UnitCodeId {
        /// The column the identity was read from.
        column: UnitCodeColumn,
        /// The identity that names no code.
        #[source]
        source: UnknownStoredUnitCode,
    },
    /// A URI value is not a URI.
    #[error("a stored attribute_history value_uri is not a URI: {0}")]
    ValueUri(#[source] UriParseError),
    /// A Number value is outside the broker's documented numeric range.
    #[error("a stored attribute_history value_number is not a representable number: {0}")]
    ValueNumber(#[source] JsonNumberError),
    /// A structured value is not the stored JSON of a value of its family.
    ///
    /// Boxed, as are [`StoredHistoryError::Members`] and
    /// [`StoredHistoryError::ProofSeals`]: the refusal is the widest member,
    /// and inlining it would enlarge every temporal store `Result`.
    #[error("a stored attribute_history value_object is not a value of its family: {0}")]
    ValueObject(#[source] Box<StoredValueError>),
    /// The stored `valueType`, `objectType` or sub-attribute elements do not
    /// assemble: a key has no name, or the elements break an invariant of the
    /// stored layout.
    #[error("the stored members of an attribute_history row do not assemble: {0}")]
    Members(#[source] Box<AssemblyError>),
    /// A stored `proof_seals` document is not the seal members of an instance.
    #[error("a stored attribute_history proof_seals is not the seals of an instance: {0}")]
    ProofSeals(#[source] Box<EntityRefusal>),
    /// A stored sub-attribute element breaks its invariants.
    #[error("a stored attribute_history sub-attribute element is unreadable: {0}")]
    SubAttribute(#[source] Box<StoredInstanceError>),
    /// A stored `value_geometry_members` document is not the `bbox` and
    /// foreign members of a geometry.
    #[error("a stored attribute_history value_geometry_members is not the members of a geometry: {0}")]
    ValueGeometryMembers(#[source] Box<EntityRefusal>),
    /// A Geometry value is not EWKB the broker writes.
    #[error("a stored attribute_history value_geometry_ewkb is not a geometry: {0}")]
    ValueGeometry(#[source] EwkbError),
    /// A scope label path does not convert back to a scope.
    #[error("a stored scope_history scope path is not a scope: {0}")]
    ScopePath(#[source] ScopePathError),
    /// The expanded `scope` IRI does not parse as an attribute name.
    #[error("the scope IRI does not name an attribute: {0}")]
    ScopeAttribute(#[source] NgsiLdReferenceError),
    /// A scope event was folded from no rows at all.
    #[error("a scope event was folded from no rows at all")]
    EmptyScopeEvent,
    /// A scope write recorded neither a path nor a removal.
    #[error("a scope write recorded neither a path nor a removal")]
    ScopeWriteWithoutValue,
}

#[cfg(test)]
mod tests {
    use crate::stored_history_error::{HistoryColumn, HistoryTable, StoredHistoryError, UnitCodeColumn};

    #[test]
    fn a_history_column_renders_as_the_name_its_statement_projects() {
        let projected: &'static str = HistoryColumn::ValueGeometryEwkb.into();
        pretty_assertions::assert_eq!(projected, "value_geometry_ewkb");
        pretty_assertions::assert_eq!(HistoryColumn::VerbatimScopePath.to_string(), "verbatim_scope_path");
        let bound: &'static str = UnitCodeColumn::UnitCodeIdLowest.into();
        pretty_assertions::assert_eq!(bound, "unit_code_id_lowest");
    }

    #[test]
    fn an_absent_column_names_the_table_and_the_column() {
        let error = StoredHistoryError::AbsentColumn {
            table: HistoryTable::ScopeHistory,
            column: HistoryColumn::DeletedAt,
        };
        pretty_assertions::assert_eq!(error.to_string(), "scope_history.deleted_at is empty but the row declares a value there");
    }
}
