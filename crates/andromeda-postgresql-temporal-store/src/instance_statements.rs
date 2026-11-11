//! The statements addressing one temporal attribute instance.
//!
//! Clause 5.6.15 addresses one instance of one temporal attribute by its
//! `instanceId`, and that identifier is stored in one of two columns: the bare
//! UUID of a broker-minted identifier, or the verbatim text of one a client
//! supplied. Every statement here addresses the column that matches the
//! identity it is given, so each reads the partial index that covers it
//! instead of an `OR` the planner has to resolve with a scan.
//!
//! The modification and the deletion bind the attribute's streams as values
//! the caller resolved first, never as a subquery: on a converted chunk
//! TimescaleDB narrows the compressed batches a statement decompresses only by
//! bound values on the segmenting stream key and the identifier's bloom
//! summary, and a subquery would have it decompress every batch whose bloom
//! summary admits the identifier, across every stream of the chunk.

use crate::{
    attribute_history_value::AttributeHistoryValue,
    horizon_raise::{horizon_raise_statement, project_horizon},
    stream_shape_growth::shape_growth_statement,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, evolution_horizon::horizon_of};
use andromeda_postgresql_schema::{
    current::Entities,
    temporal::{AttributeHistory, AttributeHistoryInstanceClaims, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    clock::now,
    literal::int_literal,
    pg_array::is_any_of,
    pg_type::PostgresType,
    postgis::{bounding_box_contains, repaired_when_invalid, st_collect, st_envelope, st_geom_from_ewkb},
};
use andromeda_postgresql_temporal_query::alias::{Ah, S, Te};
use chrono::{DateTime, Utc};
use sea_query::{Condition, DeleteStatement, Expr, ExprTrait, Func, JoinType, Query, SelectStatement, SimpleExpr, UpdateStatement};
use serde_json::value::RawValue;
use std::borrow::Cow;

/// The `attribute_history` column the identity is addressed through: the bare
/// UUID of a broker-minted identifier, or the verbatim text of a client's.
const fn identity_column(instance: &InstanceIdentity) -> AttributeHistory {
    match instance {
        InstanceIdentity::Generated(_) => AttributeHistory::GeneratedInstanceId,
        InstanceIdentity::Verbatim(_) => AttributeHistory::VerbatimInstanceId,
    }
}

/// The value the identity column is compared with.
fn identity_value(instance: &InstanceIdentity) -> SimpleExpr {
    match instance {
        InstanceIdentity::Generated(uuid) => Expr::val(*uuid),
        InstanceIdentity::Verbatim(identifier) => Expr::val(identifier.as_str()),
    }
}

/// The streams of one entity's attribute, whatever `datasetId` they carry.
///
/// An `instanceId` is unique across the attribute, so the dataset dimension is
/// not part of the address clause 5.6.15 defines; every stream of the named
/// attribute is a candidate and the instance predicate selects within them.
fn attribute_streams_query(target: &ContextTarget, entity_id: &Identifier, attribute: AttributeNameKey) -> SelectStatement {
    Query::select()
        .column((S, TemporalAttributeStreams::StreamKey))
        .from(table_of(target, TemporalAttributeStreams::Table).alias(S))
        .join_as(
            JoinType::InnerJoin,
            table_of(target, Entities::Table),
            Te,
            Expr::col((Te, Entities::EntityKey)).equals((S, TemporalAttributeStreams::EntityKey)),
        )
        .and_where(Expr::col((Te, Entities::Id)).eq(entity_id.as_str()))
        .and_where(Expr::col((S, TemporalAttributeStreams::AttributeNameKey)).eq(attribute.get()))
        .to_owned()
}

/// The stream keys of one entity's attribute.
pub(crate) fn attribute_stream_keys_statement(target: &ContextTarget, entity_id: &Identifier, attribute: AttributeNameKey) -> SelectStatement {
    attribute_streams_query(target, entity_id, attribute)
}

/// Whether the history row is in one of the resolved streams the instance is
/// sought in.
fn within_resolved_streams(stream_keys: &[i64]) -> SimpleExpr {
    is_any_of(Expr::col(AttributeHistory::StreamKey), stream_keys.iter().copied())
}

/// The instance delete, removing the addressed row rather than tombstoning it.
pub(crate) fn delete_instance_statement(instance: &InstanceIdentity, target: &ContextTarget, stream_keys: &[i64]) -> DeleteStatement {
    Query::delete()
        .from_table(table_of(target, AttributeHistory::Table))
        .and_where(Expr::col(identity_column(instance)).eq(identity_value(instance)))
        .and_where(within_resolved_streams(stream_keys))
        .to_owned()
}

/// Free the `(entity, attribute, verbatim instanceId)` claim a deleted
/// instance held, so a future write may reuse the identifier (clause 4.5.7
/// does not reserve a deleted instance's identity forever).
///
/// The claim names its attribute by dictionary key; the entity is resolved
/// through a correlated `EXISTS`, since sea-query has no `DELETE … USING`.
pub(crate) fn release_verbatim_instance_claim_statement(
    target: &ContextTarget,
    identifier: &Identifier,
    entity_id: &Identifier,
    attribute: AttributeNameKey,
) -> DeleteStatement {
    let entity_scope = Query::select()
        .expr(int_literal(1))
        .from(table_of(target, Entities::Table).alias(Te))
        .and_where(Expr::col((Te, Entities::EntityKey)).equals((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::EntityKey)))
        .and_where(Expr::col((Te, Entities::Id)).eq(entity_id.as_str()))
        .to_owned();
    Query::delete()
        .from_table(table_of(target, AttributeHistoryInstanceClaims::Table))
        .and_where(Expr::col(AttributeHistoryInstanceClaims::VerbatimInstanceId).eq(identifier.as_str()))
        .and_where(Expr::exists(entity_scope))
        .and_where(Expr::col(AttributeHistoryInstanceClaims::AttributeNameKey).eq(attribute.get()))
        .to_owned()
}

/// The geometry columns a replacement writes: `value_geometry` as written,
/// `ST_GeomFromEWKB(ewkb)`, `value_geometry_valid`, its repaired form when it
/// is invalid, and `value_geometry_reach`, `ST_GeomFromEWKB(reach)`. A NULL
/// EWKB writes NULL.
fn geometry_writes(ewkb: Option<&[u8]>, reach: Option<&[u8]>) -> [(AttributeHistory, SimpleExpr); 3] {
    let written = || st_geom_from_ewkb(Expr::val(ewkb.map(<[u8]>::to_vec)));
    [
        (AttributeHistory::ValueGeometry, written()),
        (AttributeHistory::ValueGeometryValid, repaired_when_invalid(written())),
        (AttributeHistory::ValueGeometryReach, st_geom_from_ewkb(Expr::val(reach.map(<[u8]>::to_vec)))),
    ]
}

/// A JSON text bound as `jsonb`, NULL when there is none.
fn jsonb_text(text: Option<&str>) -> SimpleExpr {
    cast(Expr::val(text.map(str::to_owned)), PostgresType::Jsonb)
}

/// What an instance replacement writes beside the sub-attributes.
pub(crate) struct InstanceReplacement<'a> {
    /// The replacement's value, in the shape and column the row stores it in.
    pub(crate) value: &'a AttributeHistoryValue,
    /// The EWKB of the replacement's geometry, if it has one.
    pub(crate) geometry: Option<&'a [u8]>,
    /// The EWKB of the geometry's great-circle reach, if it has one.
    pub(crate) reach: Option<&'a [u8]>,
    /// The observation instant the replacement declares, if any.
    pub(crate) observed_at: Option<DateTime<Utc>>,
    /// The `entity_type_names` key of `valueType`.
    pub(crate) value_type_key: Option<i32>,
    /// The stored identity of `unitCode`.
    pub(crate) unit_code: Option<i16>,
    /// The stored attribute family of the replacement.
    pub(crate) attribute_kind: i16,
    /// The `entity_type_names` keys of `objectType`, empty when it has none.
    pub(crate) object_type_keys: Vec<i32>,
    /// The replacement's `expiresAt`, if any.
    pub(crate) expires_at: Option<DateTime<Utc>>,
    /// The expression standing for the replacement's `value_number`, NULL
    /// when its value is no number.
    pub(crate) number: SimpleExpr,
}

/// The instance modification (clause 5.6.14 replacement).
///
/// The number binds directly as the column's arbitrary-precision type, so a
/// number with no exact double is stored exactly as a creation stores it.
/// `modified_at` advances the NGSI-LD axis while `recorded_at` is left alone.
/// `attribute_kind` moves with the value columns so a replacement that changes
/// family keeps the row's declared family in agreement with its value shape,
/// and every member the replacement carries beside its value replaces the
/// row's, so none of the replaced instance's survives. `sub_attributes` is
/// the expression standing for the replacement's sub-attribute elements. It
/// returns the stream of the row it replaced, which
/// [`grow_stream_extent_statement`] grows.
pub(crate) fn update_instance_statement(
    instance: &InstanceIdentity,
    target: &ContextTarget,
    stream_keys: &[i64],
    replacement: InstanceReplacement<'_>,
    sub_attributes: SimpleExpr,
) -> UpdateStatement {
    let value = replacement.value;
    let object_type_keys = (!replacement.object_type_keys.is_empty()).then_some(replacement.object_type_keys);
    Query::update()
        .table(table_of(target, AttributeHistory::Table))
        .values(geometry_writes(replacement.geometry, replacement.reach))
        .values([
            (AttributeHistory::ValueKind, Expr::val(value.kind().code())),
            (AttributeHistory::ValueNumber, replacement.number),
            (AttributeHistory::ValueString, Expr::val(value.string().map(Cow::into_owned))),
            (
                AttributeHistory::ValueLanguage,
                Expr::val(value.language().map(|language| language.as_str().to_owned())),
            ),
            (AttributeHistory::ValueDatatypeKey, Expr::val(value.datatype_key())),
            (AttributeHistory::ValueBoolean, Expr::val(value.boolean())),
            (AttributeHistory::ValueObject, jsonb_text(value.object().map(RawValue::get))),
            (AttributeHistory::ValueDateTime, Expr::val(value.date_time())),
            (AttributeHistory::ValueUri, Expr::val(value.uri().map(|uri| uri.as_str().to_owned()))),
            (AttributeHistory::ObservedAt, Expr::val(replacement.observed_at)),
            (AttributeHistory::ValueTypeKey, Expr::val(replacement.value_type_key)),
            (AttributeHistory::UnitCodeId, Expr::val(replacement.unit_code)),
            (AttributeHistory::AttributeKind, Expr::val(replacement.attribute_kind)),
            (AttributeHistory::ValueGeometryMembers, jsonb_text(value.geometry_members().map(RawValue::get))),
            (AttributeHistory::ObjectTypeKeys, Expr::val(object_type_keys)),
            (AttributeHistory::ExpiresAt, Expr::val(replacement.expires_at)),
            (AttributeHistory::SubAttributes, sub_attributes),
            (AttributeHistory::ModifiedAt, now()),
        ])
        .and_where(Expr::col(identity_column(instance)).eq(identity_value(instance)))
        .and_where(within_resolved_streams(stream_keys))
        .returning_col(AttributeHistory::StreamKey)
        .to_owned()
}

/// Raise the activity horizon of `entity_id`'s Evolution to cover the
/// instants a modification writes into its instance: the `modifiedAt` it
/// stamps and the `observedAt` the replacement declares, which clause 5.6.14
/// lets lie anywhere.
pub(crate) fn instance_horizon_statement(target: &ContextTarget, entity_id: &Identifier, observed_at: Option<DateTime<Utc>>) -> UpdateStatement {
    let mut reached = Query::select();
    reached
        .column((Te, Entities::EntityKey))
        .from(table_of(target, Entities::Table).alias(Te))
        .and_where(Expr::col((Te, Entities::Id)).eq(entity_id.as_str()));
    project_horizon(&mut reached, horizon_of(Func::greatest([now(), Expr::val(observed_at)]).into()));
    horizon_raise_statement(table_of(target, Entities::Table), reached)
}

/// Grow the extent of `stream_keys` to cover `ewkb`, the replacing geometry's
/// great-circle reach where it has one, else the geometry as written: either
/// box also bounds the repaired form a spatial predicate judges, which
/// `ST_MakeValid` builds by adding vertices on the written edges.
///
/// An extent already covering it is left alone, and none shrinks: the
/// geometry replaced stays inside an extent too large, which still holds
/// every geometry of its stream.
pub(crate) fn grow_stream_extent_statement(target: &ContextTarget, stream_keys: &[i64], ewkb: &[u8]) -> UpdateStatement {
    let extent = || Expr::col(TemporalAttributeStreams::ValueExtent);
    let geometry = || st_geom_from_ewkb(Expr::val(ewkb.to_vec()));
    Query::update()
        .table(table_of(target, TemporalAttributeStreams::Table))
        .value(
            TemporalAttributeStreams::ValueExtent,
            st_envelope(st_collect(extent(), st_envelope(geometry()))),
        )
        .and_where(is_any_of(Expr::col(TemporalAttributeStreams::StreamKey), stream_keys.iter().copied()))
        .cond_where(Condition::any().add(extent().is_null()).add(bounding_box_contains(extent(), geometry()).not()))
        .to_owned()
}

/// Grow the shape mask of the modified instance's stream, among the streams
/// `modified` the modification returned, by the combination of family and
/// value shape the stored row carries.
pub(crate) fn grow_stream_shapes_statement(instance: &InstanceIdentity, target: &ContextTarget, modified: &[i64]) -> UpdateStatement {
    let rows = Query::select()
        .from(table_of(target, AttributeHistory::Table).alias(Ah))
        .and_where(Expr::col((Ah, identity_column(instance))).eq(identity_value(instance)))
        .and_where(is_any_of(Expr::col((Ah, AttributeHistory::StreamKey)), modified.iter().copied()))
        .take();
    shape_growth_statement(table_of(target, TemporalAttributeStreams::Table), rows)
}

/// The single-instance select used for policy validation.
///
/// A read narrows the compressed batches through the join itself, so it keeps
/// the stream scope as a subquery bound by the entity id and attribute name.
pub(crate) fn select_instance_statement(
    instance: &InstanceIdentity,
    target: &ContextTarget,
    entity_id: &Identifier,
    attribute: AttributeNameKey,
) -> SelectStatement {
    Query::select()
        .column((Ah, AttributeHistory::RecordedAt))
        .column((Ah, AttributeHistory::WriteId))
        .column((Ah, AttributeHistory::WriteOrdinal))
        .column((Ah, AttributeHistory::StreamKey))
        .column((Ah, AttributeHistory::AttributeKind))
        .column((Ah, AttributeHistory::ValueKind))
        .column((Ah, AttributeHistory::ObservedAt))
        .column((Ah, AttributeHistory::DeletedAt))
        .from(table_of(target, AttributeHistory::Table).alias(Ah))
        .and_where(Expr::col((Ah, identity_column(instance))).eq(identity_value(instance)))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).in_subquery(attribute_streams_query(target, entity_id, attribute)))
        .limit(1)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_history_value::AttributeHistoryValue,
        instance_statements::{
            InstanceReplacement,
            attribute_stream_keys_statement,
            delete_instance_statement,
            grow_stream_extent_statement,
            grow_stream_shapes_statement,
            instance_horizon_statement,
            release_verbatim_instance_claim_statement,
            select_instance_statement,
            update_instance_statement,
        },
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, instance_identity::InstanceIdentity};
    use andromeda_postgresql_attribute_instance::stored_value::StoredValue;
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder};

    fn entity() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")
    }

    /// One identity per column the instance can be addressed through.
    fn identities() -> [InstanceIdentity; 2] {
        [
            InstanceIdentity::generate(),
            InstanceIdentity::Verbatim("urn:x:instance".parse().expect("a valid identifier")),
        ]
    }

    /// The update of `instance` over the streams 4 and 5, replacing it with a
    /// boolean.
    fn update(instance: &InstanceIdentity) -> (String, usize) {
        replacement_update(instance, None)
    }

    /// The update of `instance` over the streams 4 and 5, replacing it with a
    /// boolean beside `geometry`.
    fn replacement_update(instance: &InstanceIdentity, geometry: Option<&[u8]>) -> (String, usize) {
        let value = AttributeHistoryValue::Recorded(StoredValue::Boolean { flag: true, datatype: None });
        let replacement = InstanceReplacement {
            value: &value,
            geometry,
            reach: None,
            observed_at: None,
            value_type_key: None,
            unit_code: None,
            attribute_kind: 1,
            object_type_keys: Vec::new(),
            expires_at: None,
            number: Expr::val(Option::<i32>::None),
        };
        let (sql, values) = update_instance_statement(instance, &ContextTarget::Live, &[4, 5], replacement, Expr::val(0)).build(PostgresQueryBuilder);
        (sql, values.0.len())
    }

    #[test]
    fn an_instance_modification_grows_its_stream_by_the_replacing_geometry() {
        for instance in identities() {
            let (modification, _) = update(&instance);
            assert!(modification.ends_with(r#"RETURNING "stream_key""#), "{modification}");
        }
        let (growth, values) = grow_stream_extent_statement(&ContextTarget::Live, &[4, 5], &[1, 2]).build(PostgresQueryBuilder);
        assert_eq!(
            growth,
            concat!(
                r#"UPDATE "temporal_attribute_streams" SET "value_extent" = ST_Envelope(ST_Collect("value_extent", ST_Envelope(ST_GeomFromEWKB($1))))"#,
                r#" WHERE "stream_key" = ANY($2) AND ("value_extent" IS NULL OR (NOT ("value_extent" ~ ST_GeomFromEWKB($3))))"#
            )
        );
        assert_eq!(values.0.len(), 3);
    }

    /// Clause 5.6.14 lets a replacement declare any `observedAt`, so the
    /// Evolution's horizon is raised to cover it and the `modifiedAt` the
    /// modification stamps.
    #[test]
    fn an_instance_modification_raises_its_evolutions_horizon() {
        let observed = "2031-05-04T10:15:00Z".parse().expect("an instant");
        let (sql, values) = instance_horizon_statement(&ContextTarget::Live, &entity(), Some(observed)).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"UPDATE "entities" SET "evolution_active_until" = "reached"."reached""#,
                r#" FROM (SELECT "te"."entity_key", date_trunc('hour', GREATEST(NOW(), $1), 'UTC') + (make_interval(secs => $2)) AS "reached""#,
                r#" FROM "entities" AS "te" WHERE "te"."id" = $3) AS "reached""#,
                r#" WHERE "entities"."entity_key" = "reached"."entity_key""#,
                r#" AND ("entities"."evolution_active_until" IS NULL OR "entities"."evolution_active_until" < "reached"."reached")"#
            )
        );
        assert_eq!(values.0.len(), 3);
    }

    /// Clause 5.6.14 replaces the instance whole, so the modified row may
    /// carry a family and shape its stream did not hold: the combination is
    /// read off the stored row among the streams the modification returned.
    #[test]
    fn a_modified_instance_adds_its_combination_to_its_stream() {
        for instance in identities() {
            let (statement, _) = grow_stream_shapes_statement(&instance, &ContextTarget::Live, &[4]).build(PostgresQueryBuilder);
            assert!(
                statement.starts_with(r#"UPDATE "temporal_attribute_streams" SET "shape_mask" = "temporal_attribute_streams"."shape_mask" | "grown"."shape_mask" FROM (SELECT "ah"."stream_key", bit_or("#),
                "{statement}"
            );
            assert!(
                statement.contains(r#"FROM "attribute_history" AS "ah" WHERE "ah"."#) && statement.contains(r#" AND "ah"."stream_key" = $"#),
                "{statement}"
            );
        }
    }

    #[test]
    fn each_statement_addresses_exactly_one_instance_identity_column() {
        for instance in identities() {
            let target = ContextTarget::Live;
            let statements = [
                delete_instance_statement(&instance, &target, &[4]).build(PostgresQueryBuilder).0,
                update(&instance).0,
                select_instance_statement(&instance, &target, &entity(), AttributeNameKey::from(7))
                    .build(PostgresQueryBuilder)
                    .0,
                grow_stream_shapes_statement(&instance, &target, &[4]).build(PostgresQueryBuilder).0,
            ];
            for statement in statements {
                match instance {
                    InstanceIdentity::Generated(_) => {
                        assert!(statement.contains(r#""generated_instance_id" = $"#), "{statement}");
                        assert!(!statement.contains("verbatim_instance_id"), "{statement}");
                    }
                    InstanceIdentity::Verbatim(_) => {
                        assert!(statement.contains(r#""verbatim_instance_id" = $"#), "{statement}");
                        assert!(!statement.contains("generated_instance_id"), "{statement}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_stream_keys_are_resolved_from_the_named_entity_and_attribute() {
        let (statement, values) = attribute_stream_keys_statement(&ContextTarget::Live, &entity(), AttributeNameKey::from(7)).build(PostgresQueryBuilder);
        assert!(
            statement.starts_with(r#"SELECT "s"."stream_key" FROM "temporal_attribute_streams" AS "s""#),
            "{statement}"
        );
        assert!(statement.contains(r#""te"."id" = $1"#), "{statement}");
        assert!(statement.contains(r#""s"."attribute_name_key" = $2"#), "{statement}");
        assert_eq!(values.0.len(), 2);
    }

    #[test]
    fn a_modification_or_deletion_is_confined_to_the_resolved_streams_as_bound_values() {
        for instance in identities() {
            for statement in [
                delete_instance_statement(&instance, &ContextTarget::Live, &[4, 5])
                    .build(PostgresQueryBuilder)
                    .0,
                update(&instance).0,
            ] {
                assert!(statement.contains(r#""stream_key" = ANY($"#), "{statement}");
                assert!(!statement.contains("temporal_attribute_streams"), "{statement}");
            }
        }
    }

    #[test]
    fn the_policy_read_is_confined_to_the_named_entity_and_attribute() {
        for instance in identities() {
            let (statement, _) = select_instance_statement(&instance, &ContextTarget::Live, &entity(), AttributeNameKey::from(7)).build(PostgresQueryBuilder);
            assert!(statement.contains(r#"FROM "temporal_attribute_streams" AS "s""#), "{statement}");
            assert!(statement.contains(r#""te"."id" = $"#), "{statement}");
            assert!(statement.contains(r#"attribute_name_key" = $"#), "{statement}");
        }
    }

    #[test]
    fn a_modification_advances_the_modification_axis_and_leaves_the_write_clock_alone() {
        for instance in identities() {
            let (statement, values) = update(&instance);
            assert!(statement.contains(r#""modified_at" = NOW()"#), "{statement}");
            assert!(!statement.contains(r#""recorded_at" ="#), "{statement}");
            for member in [
                "value_kind",
                "attribute_kind",
                "value_type_key",
                "unit_code_id",
                "object_type_keys",
                "expires_at",
                "sub_attributes",
            ] {
                assert!(statement.contains(&format!(r#""{member}" = "#)), "{member} missing: {statement}");
            }
            assert!(values > 10, "{values}");
        }
    }

    #[test]
    fn a_deletion_removes_the_row_rather_than_recording_a_tombstone() {
        for instance in identities() {
            let (statement, _) = delete_instance_statement(&instance, &ContextTarget::Live, &[4]).build(PostgresQueryBuilder);
            assert!(statement.starts_with(r#"DELETE FROM "attribute_history""#), "{statement}");
            assert!(!statement.contains("deleted_at"), "{statement}");
        }
    }

    #[test]
    fn a_verbatim_deletion_releases_the_instance_claim_it_held() {
        let identifier: Identifier = "urn:x:instance".parse().expect("a valid identifier");
        let (statement, values) =
            release_verbatim_instance_claim_statement(&ContextTarget::Live, &identifier, &entity(), AttributeNameKey::from(7)).build(PostgresQueryBuilder);
        assert!(statement.starts_with(r#"DELETE FROM "attribute_history_instance_claims""#), "{statement}");
        assert!(statement.contains(r#""verbatim_instance_id" = $1"#), "{statement}");
        assert!(statement.contains(r#""te"."id" = $2"#), "{statement}");
        assert!(statement.contains(r#"attribute_name_key" = $3"#), "{statement}");
        assert_eq!(statement.matches("EXISTS").count(), 1, "{statement}");
        assert_eq!(values.0.len(), 3);
    }

    /// Clause 4.7.1 reads the replacing geometry back as written; clause
    /// 4.7.2 admits one RFC 7946 section 3.1.6 does not call simple, whose
    /// repair is stored beside it for spatial predicates, as is the reach a
    /// box test reads.
    #[test]
    fn the_instance_modification_writes_the_geometry_as_given_its_repair_and_its_reach() {
        for instance in identities() {
            let (statement, _) = replacement_update(&instance, Some(&[1, 2]));
            assert!(
                statement.starts_with(r#"UPDATE "attribute_history" SET "value_geometry" = ST_GeomFromEWKB($1), "value_geometry_valid" = (CASE WHEN (ST_IsValid(ST_GeomFromEWKB($2))) THEN NULL ELSE ST_MakeValid(ST_GeomFromEWKB($3)) END), "value_geometry_reach" = ST_GeomFromEWKB($4), "value_kind" = "#),
                "{statement}"
            );
        }
    }

    #[test]
    fn the_update_statement_binds_numeric_without_a_double_hop() {
        for instance in identities() {
            let (statement, _) = update(&instance);
            assert!(statement.contains(r#""value_number" = $"#), "{statement}");
            assert!(!statement.contains("double precision"), "{statement}");
        }
    }
}
