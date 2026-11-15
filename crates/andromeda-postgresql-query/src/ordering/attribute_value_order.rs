//! Ordering entities by an attribute's value (clause 4.23.1).
//!
//! Clause 4.23.1 sorts "by target value or target object", and says that "the
//! preferred sort order for numbers and datetimes is trivial, however for
//! strings the default collation order shall be defined as using ICU `root`
//! collation order". Each of those three is a key here, applied in the order
//! clause 4.23.2 ranks the datatypes in.
//!
//! An entity orders by one value: the first instance of the attribute in
//! written order (the `current_attributes` row with the lowest `ordinal`) that
//! holds the target, and on a sub-attribute path the first element in written
//! order at the end of the path. The value is read once per entity, by a
//! lateral that projects it as one typed column per kind, and every key reads
//! that projection. Which datatype the value has is decided once
//! ([`datatype_token`]), so the rank and the key that breaks its ties read one
//! answer.

use crate::{
    entity_row_refs::EntityRowRefs,
    ordering::{
        attribute_value_datatype::{NUMBER_TYPE, datatype_rank, datatype_token},
        attribute_value_holder::ordered_value,
    },
    q_predicate::{
        instance_row::ProbedRow,
        projection_probe::{Ca, of_attribute, projection_column, standing},
        sub_attribute::element_path::ElementPath,
    },
    temporal_operand::{declared_date_time, declared_time},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    filter::{instance_metadata::InstanceMetadata, path::AttrPath},
    ordering::sort_direction::SortDirection,
    standing_instant::StandingInstant,
};
use andromeda_ngsi_ld_vocabulary::{
    expanded::classes::{DATE_TIME, TIME},
    keyword,
};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{
    collation::{PostgresCollation, collate},
    literal::{false_literal, int_literal, text_literal},
    pg_operator::PgBinaryOperator,
};
use andromeda_postgresql_stored_values::resolved_names::ResolvedNames;
use sea_query::{Alias, Asterisk, CaseStatement, Condition, Expr as SeaExpr, ExprTrait, Func, Iden, IntoIden, Order, Query, SelectStatement, SimpleExpr};

/// The lateral projection one ordering term reads its attribute value from.
///
/// Each term gets its own, named after the position it holds in the `orderBy`
/// list, so a query that sorts on two attributes nests two projections without
/// either capturing the other's rows.
#[derive(Debug, Clone)]
pub struct AttributeValue {
    /// The name the enclosing statement knows the lateral by.
    alias: Alias,
}

/// The prefix each ordering term's lateral projection is aliased with.
const LATERAL_ALIAS_PREFIX: &str = "order_value_";

/// The alias the lateral reads the instance it orders by under.
const SOURCE_ALIAS: &str = "ordered";

/// The column the instance read carries a sub-attribute element in.
pub(crate) const ELEMENT_COLUMN: &str = "element";

/// The columns the lateral projects, one per kind of value, and the datatype.
#[derive(Iden, Clone, Copy)]
enum Projected {
    /// A number.
    #[iden = "value_number"]
    Number,
    /// A boolean.
    #[iden = "value_boolean"]
    Boolean,
    /// A `DateTime` instant.
    #[iden = "value_date_time"]
    DateTime,
    /// A string.
    #[iden = "value_text"]
    Text,
    /// The expanded datatype IRI the string is declared as.
    #[iden = "value_declared_datatype"]
    DeclaredDatatype,
    /// A structured value.
    #[iden = "value_object"]
    Object,
    /// The clause 4.23.2 datatype of the value.
    #[iden = "value_datatype"]
    Datatype,
}

impl AttributeValue {
    /// The projection an ordering term in `position` of the `orderBy` list
    /// reads.
    #[must_use]
    pub fn at(position: usize) -> AttributeValue {
        let mut alias = String::from(LATERAL_ALIAS_PREFIX);
        alias.push_str(&position.to_string());
        AttributeValue { alias: Alias::new(alias) }
    }

    /// The name the enclosing statement joins the projection under.
    #[must_use]
    pub fn alias(&self) -> Alias {
        // clone: sea-query takes the alias by value at each join, so every caller gets its own copy
        self.alias.clone()
    }

    /// The lateral body: the value `path` addresses on the entity `rows`
    /// names, one typed column per kind, and the clause 4.23.2 datatype it
    /// has. An entity holding none projects no row, which the enclosing left
    /// join reads as "attribute does not exist".
    #[must_use]
    pub fn lateral(&self, rows: &EntityRowRefs, path: &AttrPath, attributes: &ResolvedNames<AttributeName>) -> SelectStatement {
        let source = Alias::new(SOURCE_ALIAS);
        let read = instance_read(rows, path, attributes);
        let (element, member) = ordered_element(path, attributes, rows.standing());
        let value = ordered_value(path, element.as_ref(), member, &source);
        let mut projection = Query::select();
        projection
            .from_subquery(read, source)
            // clone: the projection consumes each column, and datatype_token still reads the ordered value afterwards
            .expr_as(value.number.clone(), Projected::Number)
            .expr_as(value.boolean.clone(), Projected::Boolean)
            .expr_as(value.date_time.clone(), Projected::DateTime)
            .expr_as(value.text.clone(), Projected::Text)
            .expr_as(value.datatype.clone(), Projected::DeclaredDatatype)
            .expr_as(value.object.clone(), Projected::Object)
            .expr_as(datatype_token(&value), Projected::Datatype);
        projection.take()
    }

    /// The clause-4.23.2 rank of this entity's value, `NULL` when the entity
    /// carries no such attribute.
    #[must_use]
    pub fn datatype_rank(&self, direction: SortDirection) -> SimpleExpr {
        datatype_rank(self.column(Projected::Datatype), direction)
    }

    /// The ordering key for numbers, `NULL` for every other datatype.
    #[must_use]
    pub fn numeric_key(&self) -> SimpleExpr {
        CaseStatement::new()
            .case(self.column(Projected::Datatype).eq(text_literal(NUMBER_TYPE)), self.column(Projected::Number))
            .into()
    }

    /// The ordering key for `DateTime` values, `NULL` for every other datatype.
    ///
    /// Clause 4.23.1 calls the sort order for datetimes trivial, which it is
    /// once each side is an instant: a value held as one is one, and a member
    /// of a structured value declared as a `DateTime` is resolved to one.
    #[must_use]
    pub fn instant_key(&self) -> SimpleExpr {
        let date_time = DATE_TIME;
        let instant = Func::coalesce([
            self.column(Projected::DateTime),
            declared_date_time(self.column(Projected::Text), self.column(Projected::DeclaredDatatype)),
        ]);
        CaseStatement::new()
            .case(self.column(Projected::Datatype).eq(text_literal(date_time.as_str())), instant)
            .into()
    }

    /// The collated ordering key for every datatype that compares as text.
    ///
    /// Strings compare as themselves, a Relationship's target object as its
    /// URI, and a `Date` as its one `YYYY-MM-DD` spelling, which is fixed-width
    /// and so already in chronological order. A `Time` compares in its
    /// fixed-width storage encoding, the one text whose byte order is
    /// chronological.
    ///
    /// An array is read through its first element. Clause 4.23.2 ranks every
    /// array under one bucket, Array, and says nothing about how two Arrays
    /// compare; the first element is what both a list and several values order
    /// by, instead of every array tying on a NULL key.
    #[must_use]
    pub fn text_key(&self, collation: &PostgresCollation) -> SimpleExpr {
        let time = TIME;
        let first = self.column(Projected::Object).json_get(int_literal(0));
        let comparable = CaseStatement::new()
            .case(
                self.column(Projected::Datatype).eq(text_literal(time.as_str())),
                declared_time(self.column(Projected::Text), self.column(Projected::DeclaredDatatype)),
            )
            .finally(Func::coalesce([
                self.column(Projected::Text),
                first.json_get_text(text_literal(keyword::VALUE)),
                self.column(Projected::Object).json_get_text(int_literal(0)),
            ]));
        collate(comparable.into(), collation)
    }

    /// One column of the projection.
    fn column(&self, column: Projected) -> SimpleExpr {
        SeaExpr::col((self.alias(), column.into_iden()))
    }
}

/// The instance of current state `path` orders by, `None` when one of its
/// names was never recorded, and the member of it the path ends on.
fn ordered_element(
    path: &AttrPath,
    attributes: &ResolvedNames<AttributeName>,
    standing: StandingInstant,
) -> (Option<ElementPath<'static>>, Option<InstanceMetadata>) {
    let (sub_attributes, member) = match path.ending_member() {
        Some((member, above)) => (above, Some(member)),
        None => (path.sub_attrs.as_slice(), None),
    };
    (
        ElementPath::of_names(&path.attr, sub_attributes, ProbedRow::Current(standing), attributes),
        member,
    )
}

/// The one instance of the entity the term orders by, read from
/// `current_attributes`: the row of the attribute with the lowest `ordinal`
/// that holds the path's target, carrying the element at the end of the path
/// when there is one.
fn instance_read(rows: &EntityRowRefs, path: &AttrPath, attributes: &ResolvedNames<AttributeName>) -> SelectStatement {
    let mut read = Query::select();
    read.column((Ca, Asterisk))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .and_where(projection_column(CurrentAttributes::EntityKey).eq(rows.entity_key_column()))
        .order_by_expr(projection_column(CurrentAttributes::Ordinal), Order::Asc)
        .limit(1);
    match ordered_element(path, attributes, rows.standing()).0 {
        // No dictionary row means no instance was ever written.
        None => {
            read.and_where(false_literal());
        }
        Some(element) => {
            read.and_where(of_attribute(element.attribute)).and_where(standing(rows));
            if let Some(first) = element.first_element() {
                read.cond_where(element.condition(Condition::all())).expr_as(first, Alias::new(ELEMENT_COLUMN));
            }
        }
    }
    read.take()
}

#[cfg(test)]
mod tests {
    use crate::{entity_row_refs::EntityRowRefs, ordering::attribute_value_order::AttributeValue};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{
        filter::{
            expression::Expr as QueryExpr,
            parser::parse,
            path::{AttrPath, QueryPath},
        },
        ordering::sort_direction::SortDirection,
    };
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, collation::PostgresCollation};
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use sea_query::{Query, SelectStatement, SimpleExpr};

    fn names() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs(
            [("speed", 7), ("address", 15), ("temperature", 12), ("accuracy", 20)]
                .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
        )
    }

    fn path(text: &str) -> AttrPath {
        let QueryExpr::Pred(pred) = parse(text).expect("a path parses as an existence query") else {
            panic!("a single predicate");
        };
        let QueryPath::Local(local) = pred.path() else {
            panic!("a local path");
        };
        local.clone()
    }

    fn lateral(text: &str) -> SelectStatement {
        AttributeValue::at(0).lateral(&EntityRowRefs::unaliased(None), &path(text), &names())
    }

    fn render(expr: SimpleExpr) -> SelectStatement {
        Query::select().expr(expr).to_owned()
    }

    /// The entity orders by the first instance of the attribute in written
    /// order, read without a document, from the typed columns.
    #[test]
    fn an_attribute_value_is_read_from_its_first_row_in_written_order() {
        assert_statement_snapshot!(
            "attribute-value-order-an-attribute-value-is-read-from-its-first-row-in-written-order",
            lateral("\"https://example.com/speed\"")
        );
    }

    /// A sub-attribute path orders by the first element at its end, read
    /// through the same chain recheck a `q` term uses.
    #[test]
    fn a_sub_attribute_value_is_read_from_the_first_element_of_the_chain() {
        assert_statement_snapshot!(
            "attribute-value-order-a-sub-attribute-value-is-read-from-the-first-element-of-the-chain",
            lateral("\"https://example.com/temperature\".\"https://example.com/accuracy\"")
        );
    }

    /// Clause 4.23.3 EXAMPLE 5: `orderBy=name.observedAt` orders by a member
    /// of the attribute, which its row holds as a typed field.
    #[test]
    fn a_member_of_the_attribute_orders_by_its_field() {
        assert_statement_snapshot!(
            "attribute-value-order-a-member-of-the-attribute-orders-by-its-field",
            lateral("\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/observedAt\"")
        );
    }

    /// Clause 4.5.2 makes `unitCode` a String: the attribute's own orders by the
    /// text of the code its stored identity names, which the text key collates
    /// like any other String.
    #[test]
    fn an_attribute_unit_code_orders_by_the_text_of_its_code() {
        assert_statement_snapshot!(
            "attribute-value-order-an-attribute-unit-code-orders-by-the-text-of-its-code",
            lateral("\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/unitCode\"")
        );
    }

    #[test]
    fn a_sub_attribute_unit_code_orders_by_the_text_of_its_code() {
        assert_statement_snapshot!(
            "attribute-value-order-a-sub-attribute-unit-code-orders-by-the-text-of-its-code",
            lateral("\"https://example.com/temperature\".\"https://example.com/accuracy\".\"https://uri.etsi.org/ngsi-ld/unitCode\"")
        );
    }

    /// A `datasetId` orders by the URI of the dataset, the default instance
    /// having none.
    #[test]
    fn a_dataset_id_orders_by_the_uri_of_the_dataset() {
        assert_statement_snapshot!(
            "attribute-value-order-a-dataset-id-orders-by-the-uri-of-the-dataset-1",
            lateral("\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/datasetId\"")
        );
        assert_statement_snapshot!(
            "attribute-value-order-a-dataset-id-orders-by-the-uri-of-the-dataset-2",
            lateral("\"https://example.com/temperature\".\"https://example.com/accuracy\".\"https://uri.etsi.org/ngsi-ld/datasetId\"")
        );
    }

    /// A `valueType` and a single `objectType` order by the expanded IRI their
    /// dictionary keys name; several types are an array with no String to
    /// order by.
    #[test]
    fn a_declared_type_orders_by_the_iri_its_key_names() {
        assert_statement_snapshot!(
            "attribute-value-order-a-declared-type-orders-by-the-iri-its-key-names-1",
            lateral("\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/hasValueType\"")
        );
        assert_statement_snapshot!(
            "attribute-value-order-a-declared-type-orders-by-the-iri-its-key-names-2",
            lateral("\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/hasObjectType\"")
        );
    }

    /// No stored instance holds an `instanceId`.
    #[test]
    fn an_instance_id_orders_as_absent() {
        assert_statement_snapshot!(
            "attribute-value-order-an-instance-id-orders-as-absent",
            lateral("\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/instanceId\"")
        );
    }

    #[test]
    fn an_instant_member_orders_by_its_instant_and_no_text() {
        assert_statement_snapshot!(
            "attribute-value-order-an-instant-member-orders-by-its-instant-and-no-text",
            lateral("\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/observedAt\"")
        );
    }

    /// Clause 4.23.3 EXAMPLE 4: `orderBy=address[city]` orders by a member of
    /// the compound value, read under each spelling clause 4.9 gives it.
    #[test]
    fn a_trailing_member_reads_every_shape_clause_4_9_gives_the_spelling() {
        assert_statement_snapshot!(
            "attribute-value-order-a-trailing-member-reads-every-shape-clause-4-9-gives-the-spelling",
            lateral("\"https://example.com/address\"[city]")
        );
    }

    /// Annex C.5.13.2: `orderBy=name[en-GB]` orders by the string of that
    /// language alone, its key compared case-insensitively (RFC 5646 section
    /// 2.1.1); no member name spells the tag.
    #[test]
    fn a_hyphenated_target_language_orders_by_that_languages_string_alone() {
        assert_statement_snapshot!(
            "attribute-value-order-a-hyphenated-target-language-orders-by-that-languages-string-alone",
            lateral("\"https://example.com/name\"[en-GB]")
        );
    }

    /// A Property stores the JSON literal `null` expanded and a `JsonProperty`
    /// stores it bare; both rank as clause 4.23.2 Null.
    #[test]
    fn an_attribute_value_reads_either_stored_json_null_as_null() {
        assert_statement_snapshot!(
            "attribute-value-order-an-attribute-value-reads-either-stored-json-null-as-null",
            lateral("\"https://example.com/speed\"")
        );
    }

    #[test]
    fn an_unrecorded_attribute_reads_no_instance() {
        assert_statement_snapshot!(
            "attribute-value-order-an-unrecorded-attribute-reads-no-instance",
            lateral("\"https://example.com/never\"")
        );
    }

    #[test]
    fn each_ordering_term_names_its_own_projection() {
        assert_statement_snapshot!(
            "attribute-value-order-each-ordering-term-names-its-own-projection-1",
            render(AttributeValue::at(0).numeric_key())
        );
        assert_statement_snapshot!(
            "attribute-value-order-each-ordering-term-names-its-own-projection-2",
            render(AttributeValue::at(1).numeric_key())
        );
    }

    #[test]
    fn every_key_reads_the_one_projected_datatype() {
        let value = AttributeValue::at(0);
        for (index, key) in [
            value.datatype_rank(SortDirection::Ascending),
            value.numeric_key(),
            value.instant_key(),
            value.text_key(&PostgresCollation::new("und-x-icu")),
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("attribute-value-order-every-key-reads-the-one-projected-datatype-{}", index + 1),
                render(key)
            );
        }
    }

    #[test]
    fn the_text_key_is_collated_and_reads_the_first_element_of_an_array() {
        assert_statement_snapshot!(
            "attribute-value-order-the-text-key-is-collated-and-reads-the-first-element-of-an-array",
            render(AttributeValue::at(0).text_key(&PostgresCollation::new("und-x-icu")))
        );
    }
}
