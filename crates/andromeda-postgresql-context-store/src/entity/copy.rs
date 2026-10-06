//! Binary COPY path for bulk entity inserts.
//!
//! Drives the Postgres binary-format [`COPY`] protocol into a transaction-
//! scoped staging table (`stage_entities`), then materialises the staged
//! rows into the three current-state relations via one `INSERT ... SELECT`
//! statement that performs the geometry projection and JSONB-array decoding
//! server-side.
//!
//! Binary COPY avoids the jsonb-array parse + per-row deserialisation
//! overhead of the `UNNEST($1::text[], $2::jsonb[], …)` path — the server
//! receives pre-typed tuples and deposits them into the staging heap
//! without any additional parsing.
//!
//! [`COPY`]: https://www.postgresql.org/docs/current/sql-copy.html

use crate::{
    entity::{
        census::delta::{CensusChange, CensusStatements},
        inserted_entity::InsertedEntity,
        mapper::stored_inserted_entity,
        params::PreparedEntityColumns,
        types_present::{entity_write_failure, types_present},
        write::{staging::Srch, type_membership::created_memberships},
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::schema_prefix;
use andromeda_postgresql_encoding::copy::{
    write_bytea_opt,
    write_field_count,
    write_header,
    write_jsonb_text,
    write_text,
    write_timestamptz_opt,
    write_trailer,
};
use sea_query::{Iden, PostgresQueryBuilder};
use sqlx::{Executor, Postgres, Transaction};

/// Number of columns written per staged entity tuple.
const STAGE_ENTITIES_FIELD_COUNT: i16 = 13;

/// DDL for the per-transaction staging table. Dropped automatically on
/// transaction commit, so concurrent chunk writes on separate pool
/// connections each get their own fresh table.
const CREATE_STAGE_ENTITIES_SQL: &str = r"
CREATE TEMP TABLE stage_entities (
    id text,
    entity_type_name_keys_literal text,
    attribute_name_keys_literal text,
    attribute_instance_name_keys_literal text,
    attribute_instance_kinds_literal text,
    scope_paths_json jsonb,
    payload jsonb,
    loc bytea,
    obs bytea,
    ops bytea,
    loc_expires_at timestamptz,
    expires_at timestamptz,
    min_attribute_expires_at timestamptz
) ON COMMIT DROP
";

/// Binary COPY statement targeting the staging table.
const COPY_INTO_STAGE_SQL: &str = "COPY stage_entities (id, entity_type_name_keys_literal, attribute_name_keys_literal, attribute_instance_name_keys_literal, attribute_instance_kinds_literal, scope_paths_json, payload, loc, obs, ops, loc_expires_at, expires_at, min_attribute_expires_at) FROM STDIN WITH (FORMAT binary)";

/// Materialise staged rows into the three entity relations of the target
/// branch, the type memberships and the discovery census beside them: the same
/// projection and the same inserts the `UNNEST` path performs, read from the
/// staging table instead of parallel arrays. The membership and census inserts
/// are the very statements the `UNNEST` path runs, rendered by the same
/// builders, so the two paths cannot count differently. The branch is selected by qualifying the relations with
/// its schema (none for the live branch, `snap_<key>` for a Snapshot branch);
/// the temporary staging table is session-scoped and needs no qualifier.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the membership insert ever stops
/// matching its column list, which its fixed shape makes unreachable.
fn insert_from_stage_sql(target: &ContextTarget) -> Result<String, ContextStoreError> {
    let schema = schema_prefix(target);
    let memberships = created_memberships(target)?.to_string(PostgresQueryBuilder);
    let census = CensusStatements::of(target, Srch, CensusChange::Added)?
        .rendered()
        .into_iter()
        .map(|(name, statement)| format!(",\n\"{}\" AS (\n    {statement}\n)", name.to_string()))
        .collect::<Vec<String>>()
        .concat();
    Ok(format!(
        r"
WITH proj AS (
    SELECT
        s.id,
        s.entity_type_name_keys_literal::integer[] AS entity_type_name_keys,
        s.attribute_name_keys_literal::integer[] AS attribute_name_keys,
        s.attribute_instance_name_keys_literal::integer[] AS attribute_instance_name_keys,
        s.attribute_instance_kinds_literal::smallint[] AS attribute_instance_kinds,
        ARRAY(SELECT jsonb_array_elements_text(s.scope_paths_json)::ltree) AS scope_paths,
        s.payload,
        CASE WHEN s.loc IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.loc)) ELSE NULL END AS location,
        CASE WHEN s.obs IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.obs)) ELSE NULL END AS observation_space,
        CASE WHEN s.ops IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(s.ops)) ELSE NULL END AS operation_space,
        s.loc_expires_at AS location_expires_at,
        s.expires_at,
        s.min_attribute_expires_at
    FROM stage_entities s
),
ins AS (
    INSERT INTO {schema}entities (id, expires_at, min_attribute_expires_at)
    SELECT proj.id, proj.expires_at, proj.min_attribute_expires_at FROM proj
    ON CONFLICT (id) DO NOTHING
    RETURNING entity_key, id
),
docs AS (
    INSERT INTO {schema}entity_documents (entity_key, payload)
    SELECT ins.entity_key, proj.payload FROM ins JOIN proj ON proj.id = ins.id
),
srch (entity_key, new_types, new_instance_names, new_instance_kinds) AS (
    INSERT INTO {schema}entity_search (entity_key, location_expires_at, entity_type_name_keys, attribute_name_keys, attribute_instance_name_keys, attribute_instance_kinds, scope_paths, location, observation_space, operation_space)
    SELECT ins.entity_key, proj.location_expires_at, proj.entity_type_name_keys, proj.attribute_name_keys, proj.attribute_instance_name_keys, proj.attribute_instance_kinds, proj.scope_paths, proj.location, proj.observation_space, proj.operation_space
    FROM ins JOIN proj ON proj.id = ins.id
    RETURNING entity_key, entity_type_name_keys, attribute_instance_name_keys, attribute_instance_kinds
),
mbrs AS (
    {memberships}
){census}
SELECT entity_key, id FROM ins
"
    ))
}

/// Bulk-insert via binary COPY into a staging table, then materialise into
/// the entity relations inside the provided transaction. Returns the rows
/// actually inserted (rows blocked by `ON CONFLICT (id) DO NOTHING` are
/// omitted), each carrying the surrogate key the projection written in the
/// same transaction is keyed on.
///
/// The inserts name their target columns explicitly, so the physical column
/// order of the relations is free to change without this statement moving.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the batch names an entity type the
/// dictionary has not interned, when the staging table cannot be created, when
/// a staged value is too long for the binary COPY length prefix or does not
/// serialise, when the binary COPY stream is rejected, when the materialising
/// insert fails, or when a returned surrogate key or identifier is not a valid
/// written entity.
pub async fn create_batch_via_copy_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    columns: &PreparedEntityColumns,
) -> Result<Vec<InsertedEntity>, ContextStoreError> {
    if columns.ids.is_empty() {
        return Ok(Vec::new());
    }
    types_present(columns)?;

    tx.execute(CREATE_STAGE_ENTITIES_SQL).await?;

    let payload = encode_entities_copy_stream(columns)?;
    {
        let mut copy = tx.copy_in_raw(COPY_INTO_STAGE_SQL).await?;
        copy.send(payload.as_slice()).await?;
        copy.finish().await?;
    }

    let insert_sql = insert_from_stage_sql(target)?;
    let rows: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(insert_sql.as_str()))
        .fetch_all(&mut **tx)
        .await
        .map_err(entity_write_failure)?;
    rows.iter().map(|(key, id)| stored_inserted_entity(*key, id)).collect()
}

/// Encode a set of entity columns into the Postgres binary COPY format.
///
/// # Errors
///
/// Returns [`ContextStoreError::CopyField`] when a staged value is longer than
/// the format's 32-bit field prefix can announce, or when a staged JSON value
/// does not serialise.
fn encode_entities_copy_stream(columns: &PreparedEntityColumns) -> Result<Vec<u8>, ContextStoreError> {
    let mut buf = Vec::with_capacity(estimated_stream_bytes(columns));
    write_header(&mut buf);
    for index in 0..columns.ids.len() {
        write_field_count(&mut buf, STAGE_ENTITIES_FIELD_COUNT);
        write_text(&mut buf, columns.ids[index].as_str())?;
        write_text(&mut buf, columns.entity_type_name_keys[index].as_str())?;
        write_text(&mut buf, columns.attribute_name_keys[index].as_str())?;
        write_text(&mut buf, columns.attribute_instance_name_keys[index].as_str())?;
        write_text(&mut buf, columns.attribute_instance_kinds[index].as_str())?;
        let scope_paths = serde_json::to_string(&columns.scope_paths_json[index]).map_err(ContextStoreError::PayloadEncoding)?;
        write_jsonb_text(&mut buf, &scope_paths)?;
        write_jsonb_text(&mut buf, &columns.payload_json[index])?;
        write_bytea_opt(&mut buf, columns.locations[index].written()?)?;
        write_bytea_opt(&mut buf, columns.obs_spaces[index].written()?)?;
        write_bytea_opt(&mut buf, columns.op_spaces[index].written()?)?;
        write_timestamptz_opt(&mut buf, columns.location_expires_ats[index]);
        write_timestamptz_opt(&mut buf, columns.expires_ats[index]);
        write_timestamptz_opt(&mut buf, columns.min_attribute_expires_ats[index]);
    }
    write_trailer(&mut buf);
    Ok(buf)
}

/// Rough byte estimate for pre-sizing the COPY buffer.
const fn estimated_stream_bytes(columns: &PreparedEntityColumns) -> usize {
    // Rough guess: 1 KiB per row accounts for typical JSON payload size.
    19 + columns.ids.len() * 1024
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        census::delta::{CensusChange, CensusStatements},
        copy::{COPY_INTO_STAGE_SQL, CREATE_STAGE_ENTITIES_SQL, STAGE_ENTITIES_FIELD_COUNT, encode_entities_copy_stream, insert_from_stage_sql},
        params::PreparedEntityColumns,
        scope_label_paths::ScopeLabelPaths,
        staged_geometry::StagedGeometry,
        write::staging::Srch,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_postgresql_codes::attribute_kind::AttributeKind;
    use andromeda_postgresql_encoding::copy::COPY_HEADER_SIGNATURE;
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
    use chrono::{DateTime, TimeZone, Utc};
    use sea_query::Iden;

    fn instant() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")
    }

    /// An entity of type `Building` with no attribute.
    fn building(id: &str) -> EntityState {
        EntityState::new(
            id.parse().expect("entity URI"),
            EntityTypes::one("https://example.org/Building".parse().expect("type name")),
        )
    }

    /// The payload field of a row is the entity state codec's own encoding,
    /// written once with the binary `jsonb` version byte in front of it.
    #[test]
    fn the_payload_field_carries_the_codec_encoding_of_the_state() {
        let stream = encode_entities_copy_stream(&sample_columns()).expect("the sample columns encode");
        let encoded = building("urn:ngsi-ld:Building:2").to_vec().expect("the state encodes");
        let mut field = vec![0x01];
        field.extend_from_slice(&encoded);
        assert!(stream.windows(field.len()).any(|window| window == field.as_slice()));
    }

    /// Build a two-row sample for encoding tests.
    fn sample_columns() -> PreparedEntityColumns {
        let mut columns = PreparedEntityColumns::with_capacity(2);
        columns.ids.push("urn:ngsi-ld:Building:1".parse().expect("entity URI"));
        columns.ids.push("urn:ngsi-ld:Building:2".parse().expect("entity URI"));
        columns.push_entity_type_name_keys([EntityTypeNameKey::from(7)]);
        columns.push_entity_type_name_keys([EntityTypeNameKey::from(7)]);
        columns.push_attribute_name_keys([AttributeNameKey::from(3), AttributeNameKey::from(4)]);
        columns.push_attribute_name_keys([]);
        columns.push_attribute_instances(vec![
            (AttributeNameKey::from(4), AttributeKind::Relationship),
            (AttributeNameKey::from(3), AttributeKind::Property),
        ]);
        columns.push_attribute_instances(Vec::new());
        columns.scope_paths_json.push(ScopeLabelPaths::new(vec!["root.a".to_owned()]));
        columns.scope_paths_json.push(ScopeLabelPaths::new(Vec::new()));
        columns.push_payload(&building("urn:ngsi-ld:Building:1")).expect("the state encodes");
        columns.push_payload(&building("urn:ngsi-ld:Building:2")).expect("the state encodes");
        let point = serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses");
        columns
            .push_geometries(
                StagedGeometry::Written(Some(point.form())),
                StagedGeometry::Written(None),
                StagedGeometry::Written(None),
            )
            .expect("geometry encodes");
        columns
            .push_geometries(StagedGeometry::Written(None), StagedGeometry::Written(None), StagedGeometry::Written(None))
            .expect("geometry encodes");
        columns.location_expires_ats.push(None);
        columns.location_expires_ats.push(None);
        columns.expires_ats.push(Some(instant()));
        columns.expires_ats.push(None);
        columns.min_attribute_expires_ats.push(None);
        columns.min_attribute_expires_ats.push(Some(instant()));
        columns
    }

    #[test]
    fn stream_starts_with_signature_and_zero_flags() {
        let stream = encode_entities_copy_stream(&sample_columns()).expect("the sample columns encode");
        assert!(stream.starts_with(COPY_HEADER_SIGNATURE));
        let flags = &stream[11..15];
        pretty_assertions::assert_eq!(flags, &[0, 0, 0, 0]);
    }

    #[test]
    fn stream_terminates_with_trailer_sentinel() {
        let stream = encode_entities_copy_stream(&sample_columns()).expect("the sample columns encode");
        pretty_assertions::assert_eq!(&stream[stream.len() - 2..], &[0xff, 0xff]);
    }

    #[test]
    fn every_row_carries_the_thirteen_staged_fields() {
        let stream = encode_entities_copy_stream(&sample_columns()).expect("the sample columns encode");
        // The first tuple's field count follows the 19-byte header.
        let field_count = u8::try_from(STAGE_ENTITIES_FIELD_COUNT).expect("the staged field count fits one byte");
        pretty_assertions::assert_eq!(&stream[19..21], &[0, field_count]);
    }

    #[test]
    fn empty_input_still_emits_header_and_trailer() {
        let empty = PreparedEntityColumns::with_capacity(0);
        let stream = encode_entities_copy_stream(&empty).expect("an empty column set encodes");
        // 11 (signature) + 4 (flags) + 4 (ext len) + 2 (trailer)
        pretty_assertions::assert_eq!(stream.len(), 21);
    }

    #[test]
    fn the_staged_columns_and_the_copy_column_list_agree() {
        for column in [
            "id",
            "entity_type_name_keys_literal",
            "attribute_name_keys_literal",
            "attribute_instance_name_keys_literal",
            "attribute_instance_kinds_literal",
            "scope_paths_json",
            "payload",
            "loc",
            "obs",
            "ops",
            "loc_expires_at",
            "expires_at",
            "min_attribute_expires_at",
        ] {
            assert!(CREATE_STAGE_ENTITIES_SQL.contains(column), "{column}");
            assert!(COPY_INTO_STAGE_SQL.contains(column), "{column}");
        }
        let live = insert_from_stage_sql(&ContextTarget::Live).expect("the fixed insert builds");
        assert!(live.contains("INSERT INTO entities (id, expires_at, min_attribute_expires_at)"));
        assert!(live.contains("INSERT INTO entity_documents (entity_key, payload)"));
        assert!(live.contains("INSERT INTO entity_search (entity_key, location_expires_at, entity_type_name_keys, attribute_name_keys"));
        assert!(live.contains("ON CONFLICT (id) DO NOTHING"));
    }

    /// Clause 4.17 selects on membership, so the COPY path lands one
    /// membership row per type exactly as the `UNNEST` path does.
    #[test]
    fn the_copy_path_lands_the_type_memberships() {
        let live = insert_from_stage_sql(&ContextTarget::Live).expect("the fixed insert builds");
        assert!(
            live.contains(r#"INSERT INTO "entity_type_members" ("entity_key", "entity_type_name_key", "id")"#),
            "{live}"
        );
        assert!(live.contains(r#"FROM "ins" JOIN "proj" ON "proj"."id" = "ins"."id""#), "{live}");
    }

    /// The COPY path counts what it inserted into the census with the very
    /// statements the `UNNEST` path renders, reading the rows its search insert
    /// returned.
    #[test]
    fn the_copy_path_counts_the_census_like_the_unnest_path() {
        let live = insert_from_stage_sql(&ContextTarget::Live).expect("the fixed insert builds");
        assert!(
            live.contains("srch (entity_key, new_types, new_instance_names, new_instance_kinds) AS ("),
            "{live}"
        );
        assert!(
            live.contains("RETURNING entity_key, entity_type_name_keys, attribute_instance_name_keys, attribute_instance_kinds"),
            "{live}"
        );
        for (name, statement) in CensusStatements::of(&ContextTarget::Live, Srch, CensusChange::Added)
            .expect("builds")
            .rendered()
        {
            let name = name.to_string();
            assert!(live.contains(&format!("\"{name}\" AS (\n    {statement}\n)")), "{name} missing: {live}");
        }
    }

    /// A Snapshot branch qualifies the target relations with its schema while
    /// the session-scoped staging table stays unqualified.
    #[test]
    fn a_branch_prefix_qualifies_the_target_relations() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(9).expect("positive key");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let branch = insert_from_stage_sql(&target).expect("the fixed insert builds");
        assert!(branch.contains("INSERT INTO snap_9_stg.entities (id"), "{branch}");
        assert!(branch.contains("INSERT INTO snap_9_stg.entity_documents (entity_key"), "{branch}");
        assert!(branch.contains("INSERT INTO snap_9_stg.entity_search (entity_key"), "{branch}");
        assert!(branch.contains(r#"INSERT INTO "snap_9_stg"."entity_type_members""#), "{branch}");
        assert!(branch.contains(r#"INSERT INTO "snap_9_stg"."attribute_census""#), "{branch}");
        assert!(branch.contains("FROM stage_entities s"), "{branch}");
    }
}
