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

use crate::attribute_value_datatype::{ARRAY_TYPE, NUMBER_TYPE, OBJECT_TYPE, OrderedValue, VALUE_MEMBER, datatype_rank, datatype_token};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{filter::path::AttrPath, ordering::sort_direction::SortDirection};
use andromeda_ngsi_ld_vocabulary::expanded::classes::{DATE_TIME, TIME};
use andromeda_postgresql_query::{
    dictionary::resolved_names::ResolvedNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        instance_row::ProbedRow,
        projection_probe::{Ca, of_attribute, projection_column, standing},
        sub_attribute::{element_path::ElementPath, member_field::MemberField},
        value_object_member::{expanded_jsonpath, is_json_property, literal_jsonpath},
    },
    temporal_operand::{stored_date_time, stored_time},
};
use andromeda_postgresql_schema::current::{CurrentAttributes, SubAttribute};
use andromeda_postgresql_seaquery::{
    cast::{cast_compound, typed_null},
    collation::{PostgresCollation, collate},
    jsonb::{escape_jsonpath_key, jsonb_build_object, jsonb_path_query_first, jsonb_typeof},
    literal::{false_literal, int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    unnest::composite_field,
};
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
const ELEMENT_COLUMN: &str = "element";

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
        // clone: sea-query takes the alias by value at the join site and every
        // key expression names it again afterwards.
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
        let value = ordered_value(path, ElementPath::resolve(path, ProbedRow::Current, attributes).as_ref(), &source);
        let mut projection = Query::select();
        projection
            .from_subquery(read, source)
            .expr_as(value.number.clone(), Projected::Number) // clone: the datatype reads the same columns
            .expr_as(value.boolean.clone(), Projected::Boolean) // clone: as above
            .expr_as(value.date_time.clone(), Projected::DateTime) // clone: as above
            .expr_as(value.text.clone(), Projected::Text) // clone: as above
            .expr_as(value.object.clone(), Projected::Object) // clone: as above
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
    /// once each side is an instant: a value held as one is one, and a string
    /// written as a `DateTime` is resolved to one.
    #[must_use]
    pub fn instant_key(&self) -> SimpleExpr {
        let date_time = DATE_TIME;
        let instant = Func::coalesce([self.column(Projected::DateTime), stored_date_time(self.column(Projected::Text))]);
        CaseStatement::new()
            .case(self.column(Projected::Datatype).eq(text_literal(date_time.as_str())), instant)
            .into()
    }

    /// The collated ordering key for every datatype that compares as text.
    ///
    /// Strings compare as themselves, a Relationship's target object as its
    /// URI, and a `Date` as its canonical `YYYY-MM-DD` form, which is
    /// fixed-width and so already in chronological order. A `Time` is
    /// normalised to a single UTC spelling first, for the reason a `DateTime`
    /// is resolved to an instant.
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
                stored_time(self.column(Projected::Text)),
            )
            .finally(Func::coalesce([
                self.column(Projected::Text),
                first.json_get_text(text_literal(VALUE_MEMBER)),
                self.column(Projected::Object).json_get_text(int_literal(0)),
            ]));
        collate(comparable.into(), collation)
    }

    /// One column of the projection.
    fn column(&self, column: Projected) -> SimpleExpr {
        SeaExpr::col((self.alias(), column.into_iden()))
    }
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
    match ElementPath::resolve(path, ProbedRow::Current, attributes) {
        // No dictionary row means no instance was ever written.
        None => {
            read.and_where(false_literal());
        }
        Some(element) => {
            read.and_where(of_attribute(element.attribute)).and_where(standing());
            if let Some(first) = element.first_element() {
                read.cond_where(element.condition(Condition::all())).expr_as(first, Alias::new(ELEMENT_COLUMN));
            }
        }
    }
    read.take()
}

/// The typed value the read instance holds for the path, read off the
/// columns of `source`.
fn ordered_value(path: &AttrPath, element: Option<&ElementPath>, source: &Alias) -> OrderedValue {
    let holder = match element.map(|element| element.chain.is_empty()) {
        Some(false) => Holder::Element(SeaExpr::col((source.clone(), Alias::new(ELEMENT_COLUMN)))),
        Some(true) | None => Holder::Row(source.clone()),
    };
    match element.and_then(|element| element.field) {
        Some(field) => holder.member(field),
        None if path.value_path.is_empty() => holder.own_value(),
        None => holder.member_of_value(path),
    }
}

/// The instance holding the ordered value: a row, or an element of a row.
enum Holder {
    /// A `current_attributes` row, read under this alias.
    Row(Alias),
    /// A `sub_attribute` element value.
    Element(SimpleExpr),
}

impl Holder {
    /// A field of a row and an element alike.
    fn field(&self, row: CurrentAttributes, element: SubAttribute) -> SimpleExpr {
        match self {
            Holder::Row(alias) => SeaExpr::col((alias.clone(), row)),
            Holder::Element(value) => composite_field(value.clone(), &element), // clone: the element is read by every field
        }
    }

    /// The holder's own value: the typed field that is set, a scalar a JSON
    /// literal holds, a structured value, and a geometry as an Object.
    fn own_value(&self) -> OrderedValue {
        let object = self.field(CurrentAttributes::ValueObject, SubAttribute::ValueObject);
        let strings = match self {
            Holder::Row(_) => vec![
                self.field(CurrentAttributes::ValueString, SubAttribute::ValueString),
                self.field(CurrentAttributes::ValueLongText, SubAttribute::ValueString),
                self.field(CurrentAttributes::ValueUri, SubAttribute::ValueUri),
                self.field(CurrentAttributes::ValueLongUri, SubAttribute::ValueUri),
                json_text(object.clone()), // clone: the object is read by every kind
            ],
            Holder::Element(_) => vec![
                self.field(CurrentAttributes::ValueString, SubAttribute::ValueString),
                self.field(CurrentAttributes::ValueUri, SubAttribute::ValueUri),
                json_text(object.clone()), // clone: as above
            ],
        };
        let geometry = CaseStatement::new().case(
            self.field(CurrentAttributes::ValueGeometry, SubAttribute::ValueGeometry).is_not_null(),
            jsonb_build_object([]),
        );
        OrderedValue {
            number: Func::coalesce([
                self.field(CurrentAttributes::ValueNumber, SubAttribute::ValueNumber),
                json_number(object.clone()),
            ])
            .into(), // clone: as above
            boolean: Func::coalesce([
                self.field(CurrentAttributes::ValueBoolean, SubAttribute::ValueBoolean),
                json_boolean(object.clone()),
            ])
            .into(), // clone: as above
            date_time: self.field(CurrentAttributes::ValueDateTime, SubAttribute::ValueDateTime),
            text: Func::coalesce(strings).into(),
            object: Func::coalesce([json_compound(object), geometry.into()]).into(),
        }
    }

    /// A member of the holder the path ends on: an instant compares as one,
    /// and a `unitCode` (held as the catalog identity of the code) has no
    /// value to order by.
    fn member(&self, field: MemberField) -> OrderedValue {
        let date_time = match field {
            MemberField::ObservedAt => self.field(CurrentAttributes::ObservedAt, SubAttribute::ObservedAt),
            MemberField::CreatedAt => self.field(CurrentAttributes::CreatedAt, SubAttribute::CreatedAt),
            MemberField::ModifiedAt => self.field(CurrentAttributes::ModifiedAt, SubAttribute::ModifiedAt),
            MemberField::ExpiresAt => self.field(CurrentAttributes::ExpiresAt, SubAttribute::ExpiresAt),
            MemberField::UnitCode => typed_null(PostgresType::Timestamptz),
        };
        OrderedValue {
            number: typed_null(PostgresType::Numeric),
            boolean: typed_null(PostgresType::Boolean),
            date_time,
            text: typed_null(PostgresType::Text),
            object: typed_null(PostgresType::Jsonb),
        }
    }

    /// The member of the holder's compound value a trailing path addresses
    /// (clause 4.23.3 EXAMPLE 4, `?orderBy=address[city]`).
    ///
    /// Clause 4.9 gives the spelling three readings and only the stored value
    /// says which applies, so each is looked up and the first that resolves is
    /// taken: a member of an expanded structured value, a member of a
    /// `JsonProperty`'s literal, and, for one member, a target language of a
    /// `languageMap`. They cannot collide: a structured value has no
    /// `@language`, a `languageMap` no IRI-keyed members, and a literal is read
    /// only on a `JsonProperty`.
    fn member_of_value(&self, path: &AttrPath) -> OrderedValue {
        let object = self.field(CurrentAttributes::ValueObject, SubAttribute::ValueObject);
        let json_property = is_json_property(self.field(CurrentAttributes::AttributeKind, SubAttribute::AttributeKind));
        let mut readings = vec![
            jsonb_path_query_first(object.clone(), &expanded_jsonpath(&path.value_path)), // clone: every reading walks the one value
            CaseStatement::new()
                .case(
                    json_property,
                    jsonb_path_query_first(object.clone(), &literal_jsonpath(&path.value_path_compact)),
                ) // clone: as above
                .into(),
        ];
        if let [language] = path.value_path_compact.as_slice() {
            readings.push(jsonb_path_query_first(object, &target_language_jsonpath(language)));
        }
        let member: SimpleExpr = Func::coalesce(readings).into();
        OrderedValue {
            number: json_number(member.clone()),   // clone: the member is read by every kind
            boolean: json_boolean(member.clone()), // clone: as above
            date_time: typed_null(PostgresType::Timestamptz),
            text: json_text(member.clone()), // clone: as above
            object: json_compound(member),
        }
    }
}

/// The jsonpath selecting the value of one language of a `languageMap`.
fn target_language_jsonpath(language: &AttributeName) -> String {
    format!("$[*] ? (@.\"@language\" == \"{}\").\"{VALUE_MEMBER}\"", escape_jsonpath_key(language.as_str()))
}

/// `json` read as a number when it is a JSON number.
fn json_number(json: SimpleExpr) -> SimpleExpr {
    json_scalar(json, NUMBER_TYPE, PostgresType::Numeric)
}

/// `json` read as a boolean when it is a JSON boolean.
fn json_boolean(json: SimpleExpr) -> SimpleExpr {
    json_scalar(json, "boolean", PostgresType::Boolean)
}

/// `json` read as text when it is a JSON string.
fn json_text(json: SimpleExpr) -> SimpleExpr {
    json_scalar(json, "string", PostgresType::Text)
}

/// `json` when it is a structure: an object or an array.
fn json_compound(json: SimpleExpr) -> SimpleExpr {
    CaseStatement::new()
        .case(
            jsonb_typeof(json.clone()).is_in([text_literal(OBJECT_TYPE), text_literal(ARRAY_TYPE)]), // clone: the test reads the value it guards
            json,
        )
        .into()
}

/// `json` as `ty` when its JSON type is `json_type`, `NULL` otherwise.
///
/// Guarded by a `CASE`, because the cast raises on a value of another type and
/// only a `CASE` guarantees the test runs first.
fn json_scalar(json: SimpleExpr, json_type: &str, ty: PostgresType) -> SimpleExpr {
    CaseStatement::new()
        .case(
            jsonb_typeof(json.clone()).eq(text_literal(json_type)), // clone: the test reads the value it guards
            cast_compound(json.json_path_text(text_literal("{}")), ty),
        )
        .into()
}

#[cfg(test)]
mod tests {
    use crate::attribute_value_order::AttributeValue;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{
        filter::{
            expression::Expr as QueryExpr,
            parser::parse,
            path::{AttrPath, QueryPath},
        },
        ordering::sort_direction::SortDirection,
    };
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames, entity_row_refs::EntityRowRefs};
    use andromeda_postgresql_seaquery::collation::PostgresCollation;
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};

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
        // clone: the test owns the path beyond the parsed expression
        local.clone()
    }

    fn lateral(text: &str) -> String {
        AttributeValue::at(0)
            .lateral(&EntityRowRefs::unaliased(None), &path(text), &names())
            .to_string(PostgresQueryBuilder)
    }

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// The entity orders by the first instance of the attribute in written
    /// order, read without a document, from the typed columns.
    #[test]
    fn an_attribute_value_is_read_from_its_first_row_in_written_order() {
        let sql = lateral("\"https://example.com/speed\"");
        assert!(
            sql.contains(r#"FROM "current_attributes" AS "ca" WHERE "ca"."entity_key" = "entities"."entity_key""#),
            "{sql}"
        );
        assert!(sql.contains(r#""ca"."attribute_name_key" = 7"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "ca"."ordinal" ASC LIMIT 1"#), "{sql}");
        assert!(sql.contains(r#""ordered"."value_number""#), "{sql}");
    }

    /// A sub-attribute path orders by the first element at its end, read
    /// through the same chain recheck a `q` term uses.
    #[test]
    fn a_sub_attribute_value_is_read_from_the_first_element_of_the_chain() {
        let sql = lateral("\"https://example.com/temperature\".\"https://example.com/accuracy\"");
        assert!(sql.contains(r#"(SELECT "l0" FROM UNNEST("ca"."sub_attributes") AS "l0""#), "{sql}");
        assert!(sql.contains(r#"("ordered"."element")."value_number""#), "{sql}");
    }

    /// Clause 4.23.3 EXAMPLE 5: `orderBy=name.observedAt` orders by a member
    /// of the attribute, which its row holds as a typed field.
    #[test]
    fn a_member_of_the_attribute_orders_by_its_field() {
        let sql = lateral("\"https://example.com/speed\".\"https://uri.etsi.org/ngsi-ld/observedAt\"");
        assert!(sql.contains(r#""ordered"."observed_at" AS "value_date_time""#), "{sql}");
    }

    /// Clause 4.23.3 EXAMPLE 4: `orderBy=address[city]` orders by a member of
    /// the compound value, read under each spelling clause 4.9 gives it.
    #[test]
    fn a_trailing_member_reads_every_shape_clause_4_9_gives_the_spelling() {
        let sql = lateral("\"https://example.com/address\"[city]");
        assert!(sql.contains(r#"'$."city"[*]."@value"'"#), "{sql}");
        assert!(sql.contains(r#"'$."city"'"#), "{sql}");
        assert!(sql.contains(r#"'$[*] ? (@."@language" == "city")."@value"'"#), "{sql}");
    }

    #[test]
    fn an_unrecorded_attribute_reads_no_instance() {
        assert!(lateral("\"https://example.com/never\"").contains("FALSE"));
    }

    #[test]
    fn each_ordering_term_names_its_own_projection() {
        let first = render(AttributeValue::at(0).numeric_key());
        let second = render(AttributeValue::at(1).numeric_key());
        assert!(first.contains("\"order_value_0\""), "{first}");
        assert!(second.contains("\"order_value_1\""), "{second}");
    }

    #[test]
    fn every_key_reads_the_one_projected_datatype() {
        let value = AttributeValue::at(0);
        for key in [
            value.datatype_rank(SortDirection::Ascending),
            value.numeric_key(),
            value.instant_key(),
            value.text_key(&PostgresCollation::new("und-x-icu")),
        ] {
            assert!(render(key).contains("\"order_value_0\".\"value_datatype\""));
        }
    }

    #[test]
    fn the_text_key_is_collated_and_reads_the_first_element_of_an_array() {
        let sql = render(AttributeValue::at(0).text_key(&PostgresCollation::new("und-x-icu")));
        assert!(sql.contains(r#"COLLATE "und-x-icu""#), "{sql}");
        assert!(sql.contains(r#"("order_value_0"."value_object" -> (0)) ->> ('@value')"#), "{sql}");
    }
}
