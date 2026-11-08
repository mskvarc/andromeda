//! Writing a registration's `information` as its `registration_information`
//! rows: the names are interned, the rows built, and the stored set replaced
//! inside the transaction that writes the registration row.
//!
//! The replacement is wholesale, one delete of the registration's rows and one
//! insert of the new set, because an update carries the whole resolved
//! `information`, so a row the update dropped must not survive it. The insert
//! stages the rows with `UNNEST`, one array per column, so its text is the same
//! for every registration. An item's names and a selector's types differ in
//! length from row to row and PostgreSQL arrays are rectangular, so each row's
//! array travels as one array-literal element and is cast where it lands.

use crate::{
    csource_registration::information_rows::{InformationRow, names_to_intern, pattern_forms_revision, rows_of},
    error::ContextStoreError,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
use andromeda_ngsi_ld_query::portable_pattern::RenderingRevision;
use andromeda_postgresql_encoding::array_literal::{IntegerArrayLiteral, TextArrayLiteral};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_schema::csr::RegistrationInformation;
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_array::nullable_array,
    pg_type::PostgresType,
    statement::Statement,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use sea_query::{DeleteStatement, Expr, ExprTrait, Iden, InsertStatement, IntoIden, Query, SimpleExpr};
use sqlx::PgConnection;
use std::collections::HashMap;

/// The `UNNEST` staging alias the rows are expanded under.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct Staged;

/// A registration's `information` as the rows it is stored as, with the keys
/// its names were interned under.
#[derive(Debug)]
pub struct PreparedInformation<'info> {
    /// The rows, in item and selector order.
    rows: Vec<InformationRow<'info>>,
    /// The key of every declared name.
    keys: HashMap<AttributeName, AttributeNameKey>,
}

impl PreparedInformation<'_> {
    /// The rendering revision the registration row records for these rows'
    /// pattern restatements; `None` when no row carries a pattern.
    #[must_use]
    pub fn pattern_forms_revision(&self) -> Option<RenderingRevision> {
        pattern_forms_revision(&self.rows)
    }

    /// The names behind the keys these rows hold, for reading the rows back
    /// inside the transaction that interned them.
    #[must_use]
    pub fn key_names(&self) -> HashMap<AttributeNameKey, AttributeName> {
        self.keys.iter().map(|(name, key)| (*key, name.clone())).collect()
    }
}

/// Intern the names `information` declares and build its rows.
///
/// Runs on the caller's connection, so the dictionary rows it mints belong to
/// the transaction that writes the registration: the rows and the names they
/// are keyed by land together or not at all.
///
/// # Errors
///
/// Returns [`ContextStoreError::NameLookup`] when the dictionary cannot resolve
/// or mint a name, and [`ContextStoreError::Invariant`] when a row cannot be
/// built from the interned keys.
pub async fn prepare_information<'info>(
    connection: &mut PgConnection,
    information: &'info [RegistrationInfo],
    attribute_names: &dyn NameLookup<AttributeName>,
) -> Result<PreparedInformation<'info>, ContextStoreError> {
    let keys = attribute_names.resolve_or_create(connection, &names_to_intern(information)).await?;
    let rows = rows_of(information, &keys)?;
    Ok(PreparedInformation { rows, keys })
}

/// The delete half of the replacement: every row of one registration.
fn delete_rows_statement(registration: &Identifier) -> DeleteStatement {
    Query::delete()
        .from_table(RegistrationInformation::Table)
        .and_where(Expr::col(RegistrationInformation::RegistrationId).eq(registration.as_str()))
        .to_owned()
}

/// The staged columns, in staging order, and the array type each binds as.
fn staged_columns() -> [UnnestColumn; 9] {
    [
        UnnestColumn::new(RegistrationInformation::ItemOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(RegistrationInformation::SelectorOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(RegistrationInformation::EntityTypes, PostgresType::TextArray),
        UnnestColumn::new(RegistrationInformation::EntityTypeShape, PostgresType::SmallintArray),
        UnnestColumn::new(RegistrationInformation::EntityId, PostgresType::TextArray),
        UnnestColumn::new(RegistrationInformation::EntityIdPattern, PostgresType::TextArray),
        UnnestColumn::new(RegistrationInformation::EntityIdPatternForm, PostgresType::TextArray),
        UnnestColumn::new(RegistrationInformation::PropertyNameKeys, PostgresType::TextArray),
        UnnestColumn::new(RegistrationInformation::RelationshipNameKeys, PostgresType::TextArray),
    ]
}

/// The insert half of the replacement: `registration` beside the rows
/// `staged` stages, one bound array per staged column.
fn insert_rows_statement(registration: &Identifier, staged: StagedColumns<'_>) -> Result<UnnestAliased<InsertStatement>, ContextStoreError> {
    let columns = staged_columns();
    let source = unnest_call(columns.iter().zip(staged.into_arrays()));
    let staged = |column: RegistrationInformation| Expr::col((Staged, column));
    let mut select = Query::select();
    select
        .expr(Expr::val(registration.as_str()))
        .expr(staged(RegistrationInformation::ItemOrdinal))
        .expr(staged(RegistrationInformation::SelectorOrdinal))
        .expr(cast(staged(RegistrationInformation::EntityTypes), PostgresType::TextArray))
        .expr(staged(RegistrationInformation::EntityTypeShape))
        .expr(staged(RegistrationInformation::EntityId))
        .expr(staged(RegistrationInformation::EntityIdPattern))
        .expr(staged(RegistrationInformation::EntityIdPatternForm))
        .expr(cast(staged(RegistrationInformation::PropertyNameKeys), PostgresType::IntegerArray))
        .expr(cast(staged(RegistrationInformation::RelationshipNameKeys), PostgresType::IntegerArray))
        .from_function(source, Staged);
    let mut insert = Query::insert();
    insert.into_table(RegistrationInformation::Table).columns([
        RegistrationInformation::RegistrationId.into_iden(),
        RegistrationInformation::ItemOrdinal.into_iden(),
        RegistrationInformation::SelectorOrdinal.into_iden(),
        RegistrationInformation::EntityTypes.into_iden(),
        RegistrationInformation::EntityTypeShape.into_iden(),
        RegistrationInformation::EntityId.into_iden(),
        RegistrationInformation::EntityIdPattern.into_iden(),
        RegistrationInformation::EntityIdPatternForm.into_iden(),
        RegistrationInformation::PropertyNameKeys.into_iden(),
        RegistrationInformation::RelationshipNameKeys.into_iden(),
    ]);
    insert.select_from(select)?;
    Ok(UnnestAliased::new(insert).with_columns(Staged, &columns))
}

/// The rows transposed into one array per staged column.
#[derive(Debug, Default)]
struct StagedColumns<'info> {
    /// `item_ordinal`.
    item_ordinals: Vec<i32>,
    /// `selector_ordinal`.
    selector_ordinals: Vec<i32>,
    /// `entity_types`, each as an array literal.
    entity_types: Vec<Option<TextArrayLiteral>>,
    /// `entity_type_shape`.
    entity_type_shapes: Vec<Option<i16>>,
    /// `entity_id`.
    entity_ids: Vec<Option<&'info str>>,
    /// `entity_id_pattern`.
    entity_id_patterns: Vec<Option<&'info str>>,
    /// `entity_id_pattern_form`.
    entity_id_pattern_forms: Vec<Option<&'info str>>,
    /// `property_name_keys`, each as an array literal.
    property_name_keys: Vec<Option<IntegerArrayLiteral>>,
    /// `relationship_name_keys`, each as an array literal.
    relationship_name_keys: Vec<Option<IntegerArrayLiteral>>,
}

impl<'info> StagedColumns<'info> {
    /// The columns of `rows`.
    fn of(rows: &[InformationRow<'info>]) -> StagedColumns<'info> {
        let mut columns = StagedColumns::default();
        for row in rows {
            let selector = row.selector.as_ref();
            columns.item_ordinals.push(row.item_ordinal);
            columns.selector_ordinals.push(row.selector_ordinal);
            columns
                .entity_types
                .push(selector.map(|selector| TextArrayLiteral::of(selector.types.iter().copied())));
            columns.entity_type_shapes.push(selector.map(|selector| selector.shape.code()));
            columns.entity_ids.push(selector.and_then(|selector| selector.id));
            columns.entity_id_patterns.push(selector.and_then(|selector| selector.pattern));
            columns.entity_id_pattern_forms.push(selector.and_then(|selector| selector.pattern_form));
            columns.property_name_keys.push(key_literal(row.property_name_keys.as_deref()));
            columns.relationship_name_keys.push(key_literal(row.relationship_name_keys.as_deref()));
        }
        columns
    }

    /// One array expression per staged column, in [`staged_columns`] order.
    fn into_arrays(self) -> Vec<SimpleExpr> {
        vec![
            Expr::val(self.item_ordinals),
            Expr::val(self.selector_ordinals),
            owned_texts(literal_texts(&self.entity_types, TextArrayLiteral::as_str)),
            nullable_array(self.entity_type_shapes),
            owned_texts(self.entity_ids),
            owned_texts(self.entity_id_patterns),
            owned_texts(self.entity_id_pattern_forms),
            owned_texts(literal_texts(&self.property_name_keys, IntegerArrayLiteral::as_str)),
            owned_texts(literal_texts(&self.relationship_name_keys, IntegerArrayLiteral::as_str)),
        ]
    }
}

/// A column of optional texts as the one `text[]` value that carries it.
fn owned_texts(column: Vec<Option<&str>>) -> SimpleExpr {
    nullable_array(column.into_iter().map(|text| text.map(str::to_owned)))
}

/// The array literal of one name axis, `None` when the axis is absent.
fn key_literal(keys: Option<&[AttributeNameKey]>) -> Option<IntegerArrayLiteral> {
    keys.map(|keys| IntegerArrayLiteral::of(keys.iter().map(|key| i64::from(key.get()))))
}

/// The texts of a column of array literals, as the statement binds them.
fn literal_texts<T>(column: &[Option<T>], text: fn(&T) -> &str) -> Vec<Option<&str>> {
    column.iter().map(|literal| literal.as_ref().map(text)).collect()
}

/// Replace a registration's information rows with `prepared`.
///
/// Runs on the caller's connection, after the registration row is written:
/// the rows reference it, and the rows and the row become visible together.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the insert cannot be built or the delete
/// or the insert fails.
pub async fn replace_registration_information(
    connection: &mut PgConnection,
    registration: &Identifier,
    prepared: &PreparedInformation<'_>,
) -> Result<(), ContextStoreError> {
    delete_rows_statement(registration).to_query().execute(&mut *connection).await?;
    if prepared.rows.is_empty() {
        return Ok(());
    }
    insert_rows_statement(registration, StagedColumns::of(&prepared.rows))?
        .to_query()
        .execute(connection)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        csource_registration::{
            information_rows::rows_of,
            information_write::{StagedColumns, insert_rows_statement, literal_texts},
        },
        statement_text::text_of,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
    use andromeda_postgresql_encoding::array_literal::{IntegerArrayLiteral, TextArrayLiteral};
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn the_insert_binds_one_typed_array_per_column() {
        let registration: Identifier = "urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid identifier");
        let sql = text_of(&insert_rows_statement(&registration, StagedColumns::default()).expect("the insert builds"));
        assert_eq!(
            sql,
            concat!(
                r#"INSERT INTO "registration_information" ("registration_id", "item_ordinal", "selector_ordinal", "entity_types", "entity_type_shape", "entity_id", "entity_id_pattern", "entity_id_pattern_form", "property_name_keys", "relationship_name_keys") "#,
                r#"SELECT $1, "d"."item_ordinal", "d"."selector_ordinal", "d"."entity_types"::text[], "d"."entity_type_shape", "d"."entity_id", "d"."entity_id_pattern", "d"."entity_id_pattern_form", "d"."property_name_keys"::integer[], "d"."relationship_name_keys"::integer[] "#,
                r#"FROM UNNEST($2::integer[], $3::integer[], $4::text[], $5::smallint[], $6::text[], $7::text[], $8::text[], $9::text[], $10::text[]) AS d(item_ordinal, selector_ordinal, entity_types, entity_type_shape, entity_id, entity_id_pattern, entity_id_pattern_form, property_name_keys, relationship_name_keys)"#,
            )
        );
    }

    #[test]
    fn three_rows_stage_as_three_elements_per_column_with_ragged_arrays_as_literals() {
        let information: Vec<RegistrationInfo> = serde_json::from_value(json!([
            {"entities": [{"type": "Building"}, {"type": ["Shop", "Bank"], "id": "urn:ngsi-ld:Shop:1", "idPattern": "^urn:ngsi-ld:Shop:.*"}], "propertyNames": ["https://example.org/p"]},
            {"relationshipNames": ["https://example.org/r", "https://example.org/s"]}
        ]))
        .expect("valid information");
        let name = |iri: &str| iri.parse::<AttributeName>().expect("valid attribute name");
        let keys = HashMap::from([
            (name("https://example.org/p"), AttributeNameKey::from(11)),
            (name("https://example.org/r"), AttributeNameKey::from(12)),
            (name("https://example.org/s"), AttributeNameKey::from(13)),
        ]);
        let rows = rows_of(&information, &keys).expect("every name is interned");
        let columns = StagedColumns::of(&rows);
        assert_eq!(columns.item_ordinals, vec![0, 0, 1]);
        assert_eq!(columns.selector_ordinals, vec![0, 1, 0]);
        assert_eq!(
            literal_texts(&columns.entity_types, TextArrayLiteral::as_str),
            vec![Some(r#"{"Building"}"#), Some(r#"{"Shop","Bank"}"#), None]
        );
        assert_eq!(columns.entity_type_shapes, vec![Some(1), Some(2), None]);
        assert_eq!(columns.entity_ids, vec![None, Some("urn:ngsi-ld:Shop:1"), None]);
        assert_eq!(columns.entity_id_patterns, vec![None, Some("^urn:ngsi-ld:Shop:.*"), None]);
        assert_eq!(columns.entity_id_pattern_forms, vec![None, Some("^urn:ngsi-ld:Shop:.*"), None]);
        assert_eq!(
            literal_texts(&columns.property_name_keys, IntegerArrayLiteral::as_str),
            vec![Some("{11}"), Some("{11}"), None]
        );
        assert_eq!(
            literal_texts(&columns.relationship_name_keys, IntegerArrayLiteral::as_str),
            vec![None, None, Some("{12,13}")]
        );
    }
}
