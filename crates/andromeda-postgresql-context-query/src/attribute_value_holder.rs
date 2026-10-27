//! The typed value an ordering term reads off the one instance it orders by
//! (clause 4.23.1): the instance's own value, a member it carries, or a member
//! of its compound value, one typed column per kind beside the datatype the
//! value is declared as.

use crate::{
    attribute_value_datatype::{ARRAY_TYPE, NULL_TYPE, NUMBER_TYPE, OBJECT_TYPE, OrderedValue},
    attribute_value_order::ELEMENT_COLUMN,
};
use andromeda_ngsi_ld_query::filter::{
    path::AttrPath,
    target_language::LanguageSelector,
    trailing_path::{MemberPath, TrailingPath},
};
use andromeda_ngsi_ld_vocabulary::{
    expanded::classes::{DATE, TIME},
    keyword,
};
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_query::q_predicate::{
    extended_filter_pattern::extended_filter_pattern,
    sub_attribute::{element_path::ElementPath, member_field::MemberField},
    value_object_member::{expanded_jsonpath, is_json_property, literal_jsonpath},
};
use andromeda_postgresql_schema::current::{CurrentAttributes, SubAttribute};
use andromeda_postgresql_seaquery::{
    cast::{cast, cast_compound, typed_null},
    jsonb::{jsonb_build_object, jsonb_path_query_first, jsonb_typeof},
    literal::{false_literal, int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    unnest::composite_field,
};
use sea_query::{Alias, CaseStatement, Expr as SeaExpr, ExprTrait, Func, SimpleExpr};

/// The typed value the read instance holds for the path, read off the
/// columns of `source`.
pub(crate) fn ordered_value(path: &AttrPath, element: Option<&ElementPath>, source: &Alias) -> OrderedValue {
    let holder = match element.map(|element| element.chain.is_empty()) {
        Some(false) => Holder::Element(SeaExpr::col((source.clone(), Alias::new(ELEMENT_COLUMN)))),
        Some(true) | None => Holder::Row(source.clone()),
    };
    match (element.and_then(|element| element.field), &path.trailing) {
        (Some(field), None | Some(TrailingPath::AllLanguages | TrailingPath::Members(_) | TrailingPath::Language(_))) => holder.member(field),
        (None, None | Some(TrailingPath::AllLanguages)) => holder.own_value(),
        (None, Some(TrailingPath::Members(members))) => holder.member_of_value(Some(members), path.target_language()),
        (None, Some(TrailingPath::Language(language))) => holder.member_of_value(None, Some(language.selector())),
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
            datatype: self.declared_datatype(),
            json_null: stored_json_null(object.clone()), // clone: as above
            object: Func::coalesce([json_compound(object), geometry.into()]).into(),
        }
    }

    /// The datatype IRI the value kind of the holder's own value declares: a
    /// `Date` or a `Time` held as text; `NULL` for every other kind.
    fn declared_datatype(&self) -> SimpleExpr {
        let kind = self.field(CurrentAttributes::ValueKind, SubAttribute::ValueKind);
        CaseStatement::new()
            .case(kind.clone().eq(int_literal(i64::from(ValueKind::Date.code()))), text_literal(DATE.as_str())) // clone: the kind is read by both arms
            .case(kind.eq(int_literal(i64::from(ValueKind::Time.code()))), text_literal(TIME.as_str()))
            .into()
    }

    /// A member of the holder the path ends on: an instant compares as one,
    /// and a `unitCode` (held as the `cefact-units` identity of the code) has no
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
            datatype: typed_null(PostgresType::Text),
            object: typed_null(PostgresType::Jsonb),
            json_null: false_literal(),
        }
    }

    /// The member of the holder's compound value a trailing path addresses
    /// (clause 4.23.3 EXAMPLE 4, `?orderBy=address[city]`), or the string of a
    /// target language.
    ///
    /// Clause 4.9 gives the spelling three readings and only the stored value
    /// says which applies, so each is looked up and the first that resolves is
    /// taken: a member of an expanded structured value, a member of a
    /// `JsonProperty`'s literal, and a target language of a `languageMap`. They
    /// cannot collide: a structured value has no `@language`, a `languageMap`
    /// no IRI-keyed members, and a literal is read only on a `JsonProperty`. A
    /// target language no member name spells (`name[en-GB]`) has the last
    /// reading alone.
    fn member_of_value(&self, members: Option<&MemberPath>, language: Option<LanguageSelector<'_>>) -> OrderedValue {
        let object = self.field(CurrentAttributes::ValueObject, SubAttribute::ValueObject);
        let json_property = is_json_property(self.field(CurrentAttributes::AttributeKind, SubAttribute::AttributeKind));
        let mut readings = Vec::new();
        let declared = match members {
            Some(members) => {
                let node_path = expanded_jsonpath(&members.addressed);
                readings.push(jsonb_path_query_first(object.clone(), &format!("{node_path}.\"{}\"", keyword::VALUE))); // clone: every reading walks the one value
                readings.push(
                    CaseStatement::new()
                        .case(json_property, jsonb_path_query_first(object.clone(), &literal_jsonpath(&members.written))) // clone: as above
                        .into(),
                );
                jsonb_path_query_first(object.clone(), &format!("{node_path}.\"{}\"", keyword::TYPE)).json_path_text(text_literal("{}")) // clone: as above
            }
            None => typed_null(PostgresType::Text),
        };
        if let Some(language) = language {
            readings.push(jsonb_path_query_first(object, &target_language_jsonpath(language)));
        }
        let member: SimpleExpr = Func::coalesce(readings).into();
        OrderedValue {
            number: json_number(member.clone()),   // clone: the member is read by every kind
            boolean: json_boolean(member.clone()), // clone: as above
            date_time: typed_null(PostgresType::Timestamptz),
            text: json_text(member.clone()), // clone: as above
            datatype: declared,
            json_null: jsonb_typeof(member.clone()).eq(text_literal(NULL_TYPE)), // clone: as above
            object: json_compound(member),
        }
    }
}

/// The jsonpath selecting the value of the `languageMap` entries a target
/// language selects.
///
/// An ordering reads a tag by tag equality, compared case-insensitively as RFC
/// 5646 section 2.1.1 requires; an extended range has no equality reading and
/// filters (RFC 4647 section 3.3.2). A validated tag or range holds letters,
/// digits and `-` only, none of which a jsonpath string or a regular
/// expression reads as syntax.
fn target_language_jsonpath(language: LanguageSelector<'_>) -> String {
    let pattern = match language {
        LanguageSelector::Tag(tag) => format!("^{}$", tag.as_str()),
        LanguageSelector::Range(_) => extended_filter_pattern(language),
    };
    format!(
        "$[*] ? (@.\"{}\" like_regex \"{pattern}\" flag \"i\").\"{}\"",
        keyword::LANGUAGE,
        keyword::VALUE
    )
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

/// Whether the stored `value_object` is the JSON literal `null`: a Property
/// stores it expanded, `{"@value": null, "@type": "@json"}`, and a
/// `JsonProperty` stores the literal itself.
fn stored_json_null(object: SimpleExpr) -> SimpleExpr {
    let expanded = jsonb_build_object([
        text_literal(keyword::VALUE),
        typed_null(PostgresType::Jsonb),
        text_literal(keyword::TYPE),
        text_literal(keyword::JSON),
    ]);
    object.is_in([expanded, cast(text_literal(NULL_TYPE), PostgresType::Jsonb)])
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
