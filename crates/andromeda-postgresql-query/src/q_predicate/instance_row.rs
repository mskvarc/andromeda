//! The one stored attribute instance a value condition reads its fields from.
//!
//! A clause 4.9 comparison is the same comparison whichever instance it reads:
//! a `current_attributes` row in the probe of a Query Entities `q`, an
//! `attribute_history` row in the probe of a temporal one, or an element of a
//! row's `sub_attributes` when the term addresses a sub-attribute. The three
//! hold a value in fields of the same names, apart from the overflow columns
//! only a row has, so a value condition is written once against an
//! [`InstanceRow`] and reads the fields of whichever one it is handed. A
//! current row and a history row both hold their sub-attributes as a tree of
//! elements in `sub_attributes`, so a path into a sub-attribute walks the tree
//! of whichever [`ProbedRow`] the probe reads.

use crate::q_predicate::{history_aliases::HistoryAliases, projection_probe::projection_column, value_column::ValueColumn};
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, SubAttribute},
    temporal::AttributeHistory,
};
use andromeda_postgresql_seaquery::{
    literal::{false_literal, int_literal},
    operator::null_if,
};
use sea_query::{Alias, Expr as SeaExpr, ExprTrait, IntoIden, SimpleExpr};

/// The top-level row a probe reads, whose `sub_attributes` a sub-attribute
/// path walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbedRow {
    /// The `current_attributes` row the value probe reads.
    Current,
    /// The `attribute_history` row a window probe reads.
    History,
}

impl ProbedRow {
    /// The row as the instance a value condition reads.
    #[must_use]
    pub const fn instance(self) -> InstanceRow {
        match self {
            ProbedRow::Current => InstanceRow::Current,
            ProbedRow::History => InstanceRow::History,
        }
    }

    /// The row's sub-attribute tree.
    #[must_use]
    pub fn sub_attributes(self) -> SimpleExpr {
        match self {
            ProbedRow::Current => projection_column(CurrentAttributes::SubAttributes),
            ProbedRow::History => HistoryAliases::top_level().instance_column(AttributeHistory::SubAttributes),
        }
    }
}

/// The stored instance a value condition reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceRow {
    /// The `current_attributes` row the value probe reads.
    Current,
    /// The `attribute_history` row a window probe reads.
    History,
    /// One element of the probed row's `sub_attributes`, unnested at this
    /// depth of the element chain.
    Element(ElementLevel),
}

/// The depth of one unnested element in a sub-attribute path, counted from
/// zero at the sub-attributes of the top-level instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementLevel(usize);

impl ElementLevel {
    /// The level `depth` elements below the top-level instance.
    #[must_use]
    pub const fn at(depth: usize) -> ElementLevel {
        ElementLevel(depth)
    }

    /// The name the element chain unnests this level under.
    #[must_use]
    pub fn alias(self) -> Alias {
        Alias::new(format!("l{}", self.0))
    }

    /// A field of the element at this level.
    #[must_use]
    pub fn field(self, field: SubAttribute) -> SimpleExpr {
        SeaExpr::col((self.alias(), field.into_iden()))
    }
}

impl InstanceRow {
    /// A value field of the instance.
    #[must_use]
    pub fn value(self, column: ValueColumn) -> SimpleExpr {
        match self {
            InstanceRow::Current => projection_column(column.in_current()),
            InstanceRow::History => HistoryAliases::top_level().instance_column(column.in_history()),
            InstanceRow::Element(level) => level.field(column.in_element()),
        }
    }

    /// The attribute family code of the instance, which every one of them
    /// records.
    #[must_use]
    pub fn attribute_family(self) -> SimpleExpr {
        match self {
            InstanceRow::Current => projection_column(CurrentAttributes::AttributeKind),
            InstanceRow::History => HistoryAliases::top_level().instance_column(AttributeHistory::AttributeKind),
            InstanceRow::Element(level) => level.field(SubAttribute::AttributeKind),
        }
    }

    /// The `entity_type_names` key of the instance's `valueType`.
    #[must_use]
    pub fn value_type_key(self) -> SimpleExpr {
        match self {
            InstanceRow::Current => projection_column(CurrentAttributes::ValueTypeKey),
            InstanceRow::History => HistoryAliases::top_level().instance_column(AttributeHistory::ValueTypeKey),
            InstanceRow::Element(level) => level.field(SubAttribute::ValueTypeKey),
        }
    }

    /// Whether the instance records the value kind `kind`, which tells apart
    /// the three kinds `value_string` holds: a String, a `Date` and a `Time`.
    ///
    /// TRUE for that kind and unknown for any other, never FALSE: a condition
    /// over another kind's column then stays unknown, as it is with no kind
    /// test at all, so the total negation `unequal` compiles to still reads a
    /// datatype difference as a match.
    #[must_use]
    pub fn value_kind_is(self, kind: ValueKind) -> SimpleExpr {
        let column = match self {
            InstanceRow::Current => projection_column(CurrentAttributes::ValueKind),
            InstanceRow::History => HistoryAliases::top_level().instance_column(AttributeHistory::ValueKind),
            InstanceRow::Element(level) => level.field(SubAttribute::ValueKind),
        };
        null_if(column.eq(int_literal(i64::from(kind.code()))), false_literal())
    }

    /// The fields a String target value can occupy, each named once.
    ///
    /// A row moves a String past the B-tree bound into its overflow column;
    /// history and an element have none and keep every String in
    /// `value_string`.
    #[must_use]
    pub const fn string_columns(self) -> &'static [ValueColumn] {
        match self {
            InstanceRow::Current => &[ValueColumn::String, ValueColumn::LongText],
            InstanceRow::History | InstanceRow::Element(_) => &[ValueColumn::String],
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        instance_row::{ElementLevel, InstanceRow, ProbedRow},
        value_column::ValueColumn,
    };
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_value_field_resolves_in_the_instance_read() {
        assert!(render(InstanceRow::Current.value(ValueColumn::LongText)).contains(r#""ca"."value_long_text""#));
        assert!(render(InstanceRow::History.value(ValueColumn::LongText)).contains(r#""qh"."value_string""#));
        assert!(render(InstanceRow::Element(ElementLevel::at(2)).value(ValueColumn::LongText)).contains(r#""l2"."value_string""#));
    }

    /// Every instance records its family, which is what decides whether its
    /// `value_object` is a `languageMap`.
    #[test]
    fn the_attribute_family_resolves_in_the_instance_read() {
        assert!(render(InstanceRow::Current.attribute_family()).contains(r#""ca"."attribute_kind""#));
        assert!(render(InstanceRow::History.attribute_family()).contains(r#""qh"."attribute_kind""#));
        assert!(render(InstanceRow::Element(ElementLevel::at(0)).attribute_family()).contains(r#""l0"."attribute_kind""#));
    }

    #[test]
    fn each_probed_row_offers_its_own_sub_attribute_tree() {
        assert!(render(ProbedRow::Current.sub_attributes()).contains(r#""ca"."sub_attributes""#));
        assert!(render(ProbedRow::History.sub_attributes()).contains(r#""qh"."sub_attributes""#));
    }
}
