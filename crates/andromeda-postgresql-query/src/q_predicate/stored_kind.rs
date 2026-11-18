//! The clause 4.9 target kinds a stored value is, by the kind of value its
//! instance records.
//!
//! A stored instance records the shape of its value as a [`ValueKind`], which
//! names the field it fills. Clause 4.9 compares a Query Term operand by the
//! data type of the target, its [`TargetKind`], so which fields a comparison
//! reads follows from the operand and these kinds alone. A JSON string
//! declared as an `xsd:float` or `xsd:double` holds the numbers beyond the
//! decimals beside the decimals in one kind, so one stored kind may be several
//! target kinds, told apart by the value. A geometry is no `q` target, a
//! structured value is read as a target holding several values or through its
//! members, and a tombstone and a seal hold no value a term compares.

use andromeda_ngsi_ld_query::filter::reading::{
    operand::{Comparison, Operand},
    target_kind::TargetKind,
};
use andromeda_postgresql_stored_values::value_kind::ValueKind;

/// The target kinds a stored value of `kind` is, empty for a kind that is no
/// single value a term compares.
#[must_use]
pub const fn stored_target_kinds(kind: ValueKind) -> &'static [TargetKind] {
    match kind {
        ValueKind::Number => &[TargetKind::Number],
        ValueKind::DeclaredNumber => &[
            TargetKind::Number,
            TargetKind::NotANumber,
            TargetKind::PositiveInfinity,
            TargetKind::NegativeInfinity,
        ],
        ValueKind::String => &[TargetKind::String],
        ValueKind::Uri => &[TargetKind::RelationshipObject],
        ValueKind::Boolean | ValueKind::DeclaredBoolean => &[TargetKind::Boolean],
        ValueKind::DateTime => &[TargetKind::DateTime],
        ValueKind::Date => &[TargetKind::Date],
        ValueKind::Time => &[TargetKind::Time],
        ValueKind::Geometry | ValueKind::Object | ValueKind::Tombstone | ValueKind::DataIntegrityProof => &[],
    }
}

/// The stored kinds a comparison of `operand` after operators of
/// `comparison` reads: each holding a target kind the operand compares with,
/// in the order the kinds are numbered.
pub fn compared_kinds(operand: &Operand, comparison: Comparison) -> impl Iterator<Item = ValueKind> {
    ValueKind::ALL
        .iter()
        .copied()
        .filter(move |kind| stored_target_kinds(*kind).iter().any(|target| operand.compares_with(*target, comparison)))
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::stored_kind::{compared_kinds, stored_target_kinds};
    use andromeda_ngsi_ld_query::filter::{
        parser::operand::{parse_date, parse_date_time, parse_time},
        reading::{
            operand::{Comparison, Operand},
            target_kind::TargetKind,
        },
    };
    use andromeda_postgresql_stored_values::value_kind::ValueKind;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    /// Every target kind a stored value can be is the kind of some stored
    /// value, but the two a raw JSON value alone holds.
    #[test]
    fn every_scalar_target_kind_is_stored_by_some_kind() {
        for target in TargetKind::iter().filter(|target| !matches!(target, TargetKind::Null | TargetKind::Object)) {
            assert!(ValueKind::ALL.iter().any(|kind| stored_target_kinds(*kind).contains(&target)), "{target:?}");
        }
    }

    /// A declared number holds the numbers beyond the decimals beside the
    /// decimals, and a geometry, a structured value, a tombstone and a seal
    /// are no single value a term compares.
    #[test]
    fn a_declared_number_is_four_kinds_and_a_structured_value_none() {
        assert_eq!(stored_target_kinds(ValueKind::DeclaredNumber).len(), 4);
        for kind in [ValueKind::Geometry, ValueKind::Object, ValueKind::Tombstone, ValueKind::DataIntegrityProof] {
            assert_eq!(stored_target_kinds(kind), &[], "{kind:?}");
        }
    }

    /// Clause 4.9 bars a Relationship's object from every operator but
    /// equality and inequality, and matches a target only with an operand of
    /// its own data type.
    #[test]
    fn each_operand_reads_the_kinds_of_its_own_data_type() {
        let text = Operand::Text("a".to_owned());
        assert_eq!(
            compared_kinds(&text, Comparison::Equality).collect::<Vec<_>>(),
            [ValueKind::String, ValueKind::Uri]
        );
        assert_eq!(compared_kinds(&text, Comparison::Order).collect::<Vec<_>>(), [ValueKind::String]);
        assert_eq!(
            compared_kinds(&Operand::Number(1.into()), Comparison::Order).collect::<Vec<_>>(),
            [ValueKind::Number, ValueKind::DeclaredNumber]
        );
        assert_eq!(
            compared_kinds(&Operand::Boolean(true), Comparison::Equality).collect::<Vec<_>>(),
            [ValueKind::Boolean, ValueKind::DeclaredBoolean]
        );
        let instant = parse_date_time("2024-01-01T00:00:00Z").expect("a date-time");
        assert_eq!(
            compared_kinds(&Operand::DateTime(instant), Comparison::Order).collect::<Vec<_>>(),
            [ValueKind::DateTime]
        );
        let date = parse_date("2024-01-01").expect("a date");
        assert_eq!(
            compared_kinds(&Operand::Date(date), Comparison::Equality).collect::<Vec<_>>(),
            [ValueKind::Date]
        );
        let time = parse_time("12:00:00Z").expect("a time");
        assert_eq!(compared_kinds(&Operand::Time(time), Comparison::Order).collect::<Vec<_>>(), [ValueKind::Time]);
    }
}
