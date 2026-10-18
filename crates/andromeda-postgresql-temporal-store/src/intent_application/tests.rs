use crate::{
    intent_application::{
        StepRole,
        application_steps,
        intent_source::{EntryReads, IntentSource, reach_statement},
    },
    temporal_entry_record::{
        TemporalIntentRecord,
        tests::{intent, keys, observation, recorded_at},
    },
};
use andromeda_context_commit::{write_id::WriteId, write_record::WriteRecord};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, scope::path::ScopePath};
use andromeda_ngsi_ld_model::temporal::TemporalValue;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    scope_entry_kind::ScopeEntryKind,
    scope_observation::ScopeObservation,
};
use pretty_assertions::assert_eq;
use sea_query::{Alias, DynIden, IntoIden, PostgresQueryBuilder};
use std::collections::BTreeSet;

/// The queue rows the drain reads, as every statement names them.
const QUEUED: &str = r#"(SELECT * FROM "temporal_append_intents" WHERE "intent_key" = ANY($1::bigint[])) AS "i""#;

/// The intents a commit binds, as every statement names them.
const BOUND: &str = concat!(
    r#"(SELECT $2::uuid AS "write_id", $3::timestamptz AS "recorded_at", "input"."entity_id", "input"."entity_type_name_keys", "#,
    r#""input"."base_ordinal", "input"."entries", "input"."ordinality" AS "intent_key" "#,
    r#"FROM UNNEST($1::temporal_intent[]) WITH ORDINALITY AS "input") AS "i""#,
);

/// The statements of an application over `source` in the branch `schema`, in
/// order.
fn steps_over(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Vec<(StepRole, String)> {
    application_steps(source, schema)
        .expect("every statement builds")
        .into_iter()
        .map(|step| (step.role, step.sql))
        .collect()
}

/// The statements of an application over the queued intents 3 and 5.
fn rendered_steps() -> Vec<(StepRole, String)> {
    steps_over(
        &IntentSource::Queued {
            keys: &[3, 5],
            reach: TemporalIntentReach::ALL.to_vec(),
        },
        None,
    )
}

/// The statements of a commit recording no intent, in the live branch.
fn commit_steps() -> Vec<(StepRole, String)> {
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    steps_over(&IntentSource::Bound { record: &record, intents: &[] }, None)
}

/// The statement among `steps` that writes into `table`.
fn writing_in(steps: Vec<(StepRole, String)>, table: &str) -> String {
    let prefix = format!(r#"INSERT INTO "{table}""#);
    steps
        .into_iter()
        .map(|(_, sql)| sql)
        .find(|sql| sql.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no statement writes {table}"))
}

/// The statement of the drain that writes into `table`.
fn writing(table: &str) -> String {
    writing_in(rendered_steps(), table)
}

/// The statement of the drain that writes the Temporal Evolutions and the
/// streams of the Entities intents state types for.
fn evolving() -> String {
    rendered_steps()
        .into_iter()
        .map(|(_, sql)| sql)
        .find(|sql| sql.starts_with(r#"WITH "evolved""#))
        .expect("an evolution statement")
}

/// The query `sql` names `name` in its `WITH` clause, up to the query that
/// follows it.
fn common_table<'s>(sql: &'s str, name: &str) -> &'s str {
    let start = sql.find(&format!(r#""{name}" "#)).unwrap_or_else(|| panic!("no {name} in {sql}"));
    let rest = &sql[start..];
    let end = [") , ", ") INSERT "]
        .iter()
        .filter_map(|follower| rest.find(follower))
        .min()
        .unwrap_or_else(|| panic!("{name} ends nowhere in {sql}"));
    &rest[..end]
}

/// The position of the first statement whose text starts with `prefix`.
/// The position of the drain's step in `role`.
fn role_position(role: StepRole) -> usize {
    rendered_steps()
        .iter()
        .position(|(step, _)| *step == role)
        .unwrap_or_else(|| panic!("no step in {role:?}"))
}

/// The statement of the drain's step in `role`.
fn in_role(role: StepRole) -> String {
    rendered_steps()
        .into_iter()
        .find(|(step, _)| *step == role)
        .map_or_else(|| panic!("no step in {role:?}"), |(_, sql)| sql)
}

fn position(prefix: &str) -> usize {
    rendered_steps()
        .iter()
        .position(|(_, sql)| sql.starts_with(prefix))
        .unwrap_or_else(|| panic!("no statement starts with {prefix}"))
}

/// The record `intent` is bound as.
fn record_of(intent: &TemporalAppendIntent) -> TemporalIntentRecord {
    TemporalIntentRecord::of(intent, &keys(intent)).expect("the intent is storable")
}

/// Which needs a commit binding `intents` meets, in registry order.
fn needs_met(intents: &[TemporalIntentRecord]) -> Vec<bool> {
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    let source = IntentSource::Bound { record: &record, intents };
    TemporalIntentReach::ALL.iter().map(|need| source.may_meet(*need)).collect()
}

/// The fields of the entry under `e` that `sql` reads, its position aside:
/// the unnest numbers every entry whatever the bound form holds.
fn entry_fields_read(sql: &str) -> BTreeSet<&str> {
    sql.match_indices(r#""e"."#)
        .filter_map(|(at, marker)| sql[at + marker.len()..].strip_prefix('"'))
        .filter_map(|rest| rest.split('"').next())
        .filter(|field| *field != "ordinality")
        .collect()
}

/// The end of the default instance of `speed`.
fn tombstone() -> TemporalAppendEntry {
    TemporalAppendEntry::Tombstone {
        attribute_name: "https://example.com/speed".parse().expect("an attribute name"),
        dataset_id: None,
        attribute_type: AttributeType::Property,
        instance: InstanceIdentity::generate(),
    }
}

#[test]
fn the_application_reads_the_entries_from_the_queue_rows_it_holds() {
    let history = writing("attribute_history");
    assert!(
        history.contains(&format!(r#"FROM {QUEUED}, UNNEST("i"."entries") WITH ORDINALITY AS "e""#)),
        "{history}"
    );
    for (_, sql) in rendered_steps() {
        assert!(sql.contains(QUEUED), "every statement reads the claimed rows: {sql}");
        assert!(!sql.contains("payload") && !sql.contains("::json"), "{sql}");
    }
}

/// The source relation `relation` names under `i`, as the Scope judgement
/// reads it again under `sj`.
fn earlier(relation: &str) -> String {
    relation.replace(r#"AS "i""#, r#"AS "sj""#)
}

/// The part of the guard of a Scope assertion only the drain renders: the
/// queued intents it holds claimed are in its source, and every intent queued
/// when a commit records its own came before it.
const DRAIN_GUARD: &str = r#" AND "sq"."intent_key" < "i"."intent_key" AND "sq"."intent_key" <> ALL($1::bigint[])"#;

#[test]
fn the_commit_and_the_drain_render_one_statement_shape() {
    let drain: Vec<(StepRole, String)> = rendered_steps()
        .into_iter()
        .map(|(role, sql)| {
            (
                role,
                sql.replace(QUEUED, "<source>").replace(&earlier(QUEUED), "<source>").replace(DRAIN_GUARD, ""),
            )
        })
        .collect();
    let commit: Vec<(StepRole, String)> = commit_steps()
        .into_iter()
        .map(|(role, sql)| (role, sql.replace(BOUND, "<source>").replace(&earlier(BOUND), "<source>")))
        .collect();
    assert_eq!(drain, commit);
    assert!(drain.iter().all(|(_, sql)| sql.contains("<source>")), "{drain:?}");
}

#[test]
fn a_commit_binds_its_intents_as_one_typed_array() {
    for (_, sql) in commit_steps() {
        assert!(sql.contains(BOUND), "{sql}");
        assert_eq!(
            sql.matches("$1::temporal_intent[]").count(),
            sql.matches(BOUND).count() + sql.matches(&earlier(BOUND)).count(),
            "{sql}"
        );
        let queue_reads = sql.matches("temporal_append_intents").count();
        assert_eq!(queue_reads, sql.matches(r#"FROM "temporal_append_intents" AS "sq""#).count(), "{sql}");
    }
}

/// The Scope insert of the drain.
fn scope_insert() -> String {
    writing("scope_history")
}

/// Clause 4.18: an assertion of the Scope already in force sets nothing, so
/// it is inserted only when the state at the newest event at or before its
/// write differs on the `recorded_at` or the `observed_at` axis; the other
/// two axes order every event as `recorded_at` does.
#[test]
fn an_assertion_is_inserted_only_when_either_head_differs() {
    let sql = scope_insert();
    for axis in ["recorded_at", "observed_at"] {
        assert!(
            sql.contains(&format!(r#"WHERE "sah"."{axis}" <= "i"."recorded_at" ORDER BY "sah"."{axis}" DESC LIMIT 1"#)),
            "{axis}: {sql}"
        );
    }
    assert!(
        sql.contains(r#"FROM "scope_history" AS "sr" WHERE "sr"."temporal_entity_key" = "te"."temporal_entity_key""#),
        "{sql}"
    );
    assert!(
        sql.contains(r#"FROM UNNEST("i"."entries") AS "sa" WHERE "sa"."entry_kind" = 4 ORDER BY ("sa"."verbatim_scope_path") COLLATE "C" ASC"#),
        "{sql}"
    );
    assert_eq!(sql.matches(r#"COLLATE "C") = ((ARRAY("#).count(), 2, "{sql}");
}

/// A change of Scope, a reified instance and a removal are events whatever
/// the history holds: only the assertion kind is judged.
#[test]
fn a_change_an_instance_and_a_removal_are_inserted_unconditionally() {
    let sql = scope_insert();
    assert!(sql.contains(r#""e"."entry_kind" IN (3, 4)"#), "{sql}");
    assert!(
        sql.contains(r#"("e"."entry_kind" <> 4 OR EXISTS(SELECT 1 FROM "temporal_append_intents" AS "sq""#),
        "{sql}"
    );
}

/// The statement cannot see the rows it inserts, so an assertion is judged
/// against the Scope entries of its Entity's intents before its own in the
/// batch as well as the recorded events: one equal to the Scope an earlier
/// entry of the batch set is dropped, and one an earlier entry changed is
/// recorded.
#[test]
fn an_assertion_is_judged_against_the_earlier_scope_entries_of_its_entity_in_the_batch() {
    let sql = scope_insert();
    assert!(
        sql.contains(&format!(
            r#"UNION ALL (SELECT "sj"."recorded_at" AS "recorded_at", "se"."observed_at" AS "observed_at", "se"."verbatim_scope_path" AS "verbatim_scope_path" FROM {}, UNNEST("sj"."entries") AS "se" WHERE "sj"."entity_id" = "i"."entity_id" AND "sj"."intent_key" < "i"."intent_key" AND "se"."entry_kind" IN (3, 4))"#,
            earlier(QUEUED)
        )),
        "{sql}"
    );
}

/// Both heads read the recorded events and the earlier entries of the batch
/// together, so the first assertion of an Entity in a batch is judged against
/// the recorded heads alone.
#[test]
fn the_first_assertion_of_an_entity_in_the_batch_is_judged_against_both_heads() {
    let sql = scope_insert();
    let union = r#"FROM "scope_history" AS "sr" WHERE "sr"."temporal_entity_key" = "te"."temporal_entity_key" UNION ALL"#;
    assert_eq!(sql.matches(union).count(), 4, "two reads of each of two axes: {sql}");
}

/// Intents are claimed in key order across workers and a retry reschedules
/// one, so an earlier intent of the Entity may still be queued outside the
/// batch, or applied by a transaction not yet committed: the assertion is
/// inserted unjudged. A commit recording its own intents treats every queued
/// intent of its Entity as earlier, and a Snapshot branch has no queue.
#[test]
fn an_assertion_with_an_earlier_intent_of_its_entity_still_queued_is_inserted() {
    let drain = scope_insert();
    assert!(
        drain.contains(&format!(
            r#"OR EXISTS(SELECT 1 FROM "temporal_append_intents" AS "sq" WHERE "sq"."entity_id" = "i"."entity_id"{DRAIN_GUARD}) OR"#
        )),
        "{drain}"
    );
    let commit = writing_in(commit_steps(), "scope_history");
    assert!(
        commit.contains(r#"OR EXISTS(SELECT 1 FROM "temporal_append_intents" AS "sq" WHERE "sq"."entity_id" = "i"."entity_id") OR"#),
        "{commit}"
    );
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    let schema = Alias::new("snap_1").into_iden();
    let snapshot = writing_in(
        steps_over(&IntentSource::Bound { record: &record, intents: &[] }, Some(&schema)),
        r#"snap_1"."scope_history"#,
    );
    assert!(!snapshot.contains("temporal_append_intents"), "{snapshot}");
}

#[test]
fn a_write_record_is_persisted_once_per_commit() {
    let writes: Vec<String> = commit_steps()
        .into_iter()
        .map(|(_, sql)| sql)
        .filter(|sql| sql.starts_with(r#"INSERT INTO "write_records""#))
        .collect();
    let [write] = writes.as_slice() else {
        panic!("one write-record insert: {writes:?}");
    };
    assert!(
        write.contains(r#"("write_id", "recorded_at", "accepted_at") SELECT DISTINCT "i"."write_id", "i"."recorded_at", NOW()"#),
        "{write}"
    );
    assert!(write.ends_with("ON CONFLICT  DO NOTHING"), "{write}");
}

/// Clause 5.6.16 deletes a Temporal Evolution on its own, so the end of an
/// instance a commit records can reach an Entity that has none, and is
/// recorded nowhere: the drain's join decides it.
#[test]
fn a_native_deletion_of_an_entity_without_evolution_records_nothing() {
    for table in ["attribute_history", "scope_history", "temporal_attribute_streams"] {
        let sql = writing_in(commit_steps(), table);
        assert!(sql.contains(r#""temporal_entities" AS "te""#), "{sql}");
        assert!(sql.contains(r#""te"."id" = "i"."entity_id""#), "{sql}");
    }
}

/// A Snapshot branch records its history in its own relations; the write
/// records and the queue belong to the tenant.
#[test]
fn a_commit_to_a_snapshot_branch_records_in_its_schema() {
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    let schema = Alias::new("snap_7").into_iden();
    let steps = steps_over(&IntentSource::Bound { record: &record, intents: &[] }, Some(&schema));
    for table in [
        "temporal_entities",
        "temporal_attribute_streams",
        "attribute_history",
        "attribute_history_instance_claims",
        "scope_history",
    ] {
        assert!(
            steps.iter().any(|(_, sql)| sql.contains(&format!(r#""snap_7"."{table}""#))),
            "{table}: {steps:?}"
        );
    }
    assert!(steps.iter().any(|(_, sql)| sql.starts_with(r#"INSERT INTO "write_records""#)), "{steps:?}");
}

/// A statement no entry of a commit reaches is not run: a typed observation
/// with a broker-minted instance needs no claim and no Scope row.
#[test]
fn a_commit_runs_only_the_statements_its_entries_reach() {
    let typed = record_of(&intent(vec![observation(
        TemporalValue::Number(JsonNumber::from(42)),
        InstanceIdentity::generate(),
        None,
    )]));
    assert_eq!(needs_met(&[typed]), [true, false, true, false, false, false]);

    let claimed = record_of(&intent(vec![observation(
        TemporalValue::Number(JsonNumber::from(42)),
        InstanceIdentity::Verbatim("urn:x".parse().expect("an instance identifier")),
        None,
    )]));
    let scoped = record_of(&intent(vec![TemporalAppendEntry::Scope {
        observation: ScopeObservation::Path("/Madrid".parse::<ScopePath>().expect("a scope path")),
        kind: ScopeEntryKind::Event,
        observed_at: None,
    }]));
    assert_eq!(needs_met(&[claimed, scoped]), [true, false, true, true, true, false]);
}

/// An intent stating no type observes, and is checked against its
/// evolution, unless every entry it holds ends an instance.
#[test]
fn only_an_untyped_intent_that_observes_reaches_the_pre_check() {
    let untyped = |entries: Vec<TemporalAppendEntry>| {
        let mut untyped = intent(entries);
        untyped.entity_types.clear();
        record_of(&untyped)
    };
    assert_eq!(needs_met(&[untyped(vec![tombstone()])])[1], false);
    assert_eq!(needs_met(&[untyped(Vec::new())])[1], true);
    let observing = untyped(vec![
        tombstone(),
        observation(TemporalValue::Number(JsonNumber::from(1)), InstanceIdentity::generate(), None),
    ]);
    assert_eq!(needs_met(&[observing])[1], true);
}

/// The drain runs the statements its claimed intents reach, as their flags
/// say, and nothing else.
#[test]
fn a_queued_source_runs_the_statements_its_claimed_intents_reach() {
    let reach = vec![TemporalIntentReach::TypedIntent, TemporalIntentReach::AttributeEntry];
    let source = IntentSource::Queued { keys: &[1], reach };
    let met: Vec<bool> = TemporalIntentReach::ALL.iter().map(|need| source.may_meet(*need)).collect();
    assert_eq!(met, [true, false, true, false, false, false]);
}

/// The reach of a claimed batch comes from the rows' `entry_needs`, each
/// code once, and never from the entries.
#[test]
fn the_reach_of_claimed_intents_is_read_without_their_entries() {
    let sql = reach_statement().to_string(PostgresQueryBuilder);
    assert_eq!(
        sql,
        r#"SELECT DISTINCT UNNEST("entry_needs") FROM "temporal_append_intents" WHERE "intent_key" = ANY($1::bigint[])"#
    );
}

/// A bound source sends each statement only the entry fields it reads, so a
/// statement declared to read less must read no more.
#[test]
fn each_statement_reads_no_entry_field_beyond_what_it_is_bound() {
    let identity = BTreeSet::from([
        "entry_kind",
        "attribute_name_key",
        "dataset_id",
        "generated_instance_id",
        "verbatim_instance_id",
    ]);
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    for step in application_steps(&IntentSource::Bound { record: &record, intents: &[] }, None).expect("every statement builds") {
        let read = entry_fields_read(&step.sql);
        match step.reads {
            EntryReads::Nothing => assert!(read.is_empty() && !step.sql.contains(r#"UNNEST("i"."entries")"#), "{}", step.sql),
            EntryReads::Identities => assert!(read.is_subset(&identity), "{read:?} in {}", step.sql),
            EntryReads::Everything => assert!(!read.is_empty(), "{}", step.sql),
        }
    }
}

/// The light forms a bound source sends keep what the statements reading
/// them read, and drop every value.
#[test]
fn a_bound_head_drops_every_entry_and_an_identity_every_value() {
    let full = record_of(&intent(vec![observation(
        TemporalValue::Number(JsonNumber::from(42)),
        InstanceIdentity::Verbatim("urn:x".parse().expect("an instance identifier")),
        None,
    )]));
    let head = full.head();
    assert_eq!(
        (&head.entity_id, &head.entity_type_name_keys, head.base_ordinal, head.entries.len()),
        (&full.entity_id, &full.entity_type_name_keys, full.base_ordinal, 0)
    );
    assert_eq!(
        head.entry_needs,
        [
            TemporalIntentReach::TypedIntent,
            TemporalIntentReach::AttributeEntry,
            TemporalIntentReach::VerbatimInstance
        ]
        .map(TemporalIntentReach::code)
    );
    let identities = full.identities();
    let ([identity], [entry]) = (identities.entries.as_slice(), full.entries.as_slice()) else {
        panic!("one entry each: {identities:?}");
    };
    assert_eq!(
        (identity.entry_kind, identity.attribute_name_key, &identity.dataset_id, identity.value_kind),
        (entry.entry_kind, entry.attribute_name_key, &entry.dataset_id, entry.value_kind)
    );
    assert_eq!(
        (&identity.verbatim_instance_id, identity.generated_instance_id),
        (&entry.verbatim_instance_id, entry.generated_instance_id)
    );
    assert!(
        entry.value_number.is_some() && entry.observed_at.is_some(),
        "the fixture carries a value and an instant"
    );
    assert!(identity.value_number.is_none() && identity.observed_at.is_none() && identity.created_at.is_none() && identity.sub_attributes.is_none());
}

#[test]
fn missing_evolutions_and_streams_are_created_before_the_history() {
    let evolutions = position(r#"WITH "evolved""#);
    let untyped_streams = position(r#"INSERT INTO "temporal_attribute_streams""#);
    let history = position(r#"INSERT INTO "attribute_history""#);
    let scopes = position(r#"INSERT INTO "scope_history""#);
    assert!(
        evolutions < untyped_streams && untyped_streams < history && evolutions < scopes,
        "{:?}",
        rendered_steps()
    );
    assert!(position(r#"INSERT INTO "write_records""#) < history);
    assert!(role_position(StepRole::Replace) < history, "the replaced instance goes first");
    assert!(position(r#"INSERT INTO "attribute_history_instance_claims""#) < history);
}

/// A source may hold several intents of one Entity, and a conflict action
/// changes a row once per statement, so the evolution takes the types of the
/// Entity's latest typed intent in one row.
#[test]
fn the_latest_typed_intent_of_an_entity_gives_its_evolution_its_types() {
    let evolving = evolving();
    let evolutions = common_table(&evolving, "evolved");
    assert!(evolutions.contains(r#"AS (INSERT INTO "temporal_entities""#), "{evolutions}");
    assert!(evolutions.contains(r#"SELECT DISTINCT ON ("i"."entity_id")"#), "{evolutions}");
    assert!(evolutions.contains(r#"ORDER BY "i"."entity_id" ASC, "i"."intent_key" DESC"#), "{evolutions}");
    assert!(evolutions.contains(r#"cardinality("i"."entity_type_name_keys") > 0"#), "{evolutions}");
    assert!(
        evolutions.contains(r#"DO UPDATE SET "modified_at" = "excluded"."modified_at", "entity_type_name_keys" = "excluded"."entity_type_name_keys""#),
        "{evolutions}"
    );
}

/// The membership moves read the types the evolution had and has from the
/// upsert's own images: the replaced row is the one it found after waiting on
/// a concurrent writer, which the statement's snapshot may predate.
#[test]
fn an_evolution_returns_the_types_it_had_and_has() {
    let evolving = evolving();
    let evolutions = common_table(&evolving, "evolved");
    assert!(
        evolutions.starts_with(r#""evolved" ("temporal_entity_key", "id", "created", "new_types", "old_types") AS"#),
        "{evolutions}"
    );
    assert!(
        evolutions.ends_with(r#"RETURNING "new"."temporal_entity_key", "new"."id", "old"."temporal_entity_key" IS NULL, "new"."entity_type_name_keys", "old"."entity_type_name_keys""#),
        "{evolutions}"
    );
}

/// Clause 4.17 selects on membership: an evolution the statement created
/// lands a row for every type, and an updated one for each type its old types
/// lacked.
#[test]
fn an_evolution_lands_a_membership_row_for_every_type_it_gained() {
    let evolving = evolving();
    let added = common_table(&evolving, "added_members");
    assert!(
        added.contains(r#"AS (INSERT INTO "temporal_entity_type_members" ("temporal_entity_key", "entity_type_name_key", "id") SELECT "evolved"."temporal_entity_key", "carried"."entity_type_name_key", "evolved"."id" FROM "evolved""#),
        "{added}"
    );
    assert!(added.contains(r#"UNNEST("evolved"."new_types")"#), "{added}");
    assert!(
        added.contains(r#"WHERE "evolved"."created" OR (NOT "carried"."entity_type_name_key" = ANY("evolved"."old_types"))"#),
        "{added}"
    );
}

/// A type the latest typed intent no longer states leaves the membership
/// relation, so the evolution stops answering a selection of that type.
#[test]
fn an_evolution_drops_the_membership_rows_of_the_types_it_lost() {
    let evolving = evolving();
    let dropped = common_table(&evolving, "dropped_members");
    assert!(
        dropped.contains(r#"AS (DELETE FROM "temporal_entity_type_members" WHERE ("entity_type_name_key", "id") IN (SELECT "carried"."entity_type_name_key", "evolved"."id" FROM "evolved""#),
        "{dropped}"
    );
    assert!(dropped.contains(r#"UNNEST("evolved"."old_types")"#), "{dropped}");
    assert!(
        dropped.contains(r#"NOT "carried"."entity_type_name_key" = ANY("evolved"."new_types")"#),
        "{dropped}"
    );
}

/// A Snapshot records its Temporal Evolutions in its own schema, memberships
/// included.
#[test]
fn a_snapshot_moves_its_memberships_in_its_own_schema() {
    let schema = Alias::new("snap_4").into_iden();
    let evolving = steps_over(
        &IntentSource::Queued {
            keys: &[3, 5],
            reach: TemporalIntentReach::ALL.to_vec(),
        },
        Some(&schema),
    )
    .into_iter()
    .map(|(_, sql)| sql)
    .find(|sql| sql.starts_with(r#"WITH "evolved""#))
    .expect("an evolution statement");
    assert!(evolving.contains(r#"INSERT INTO "snap_4"."temporal_entity_type_members""#), "{evolving}");
    assert!(evolving.contains(r#"DELETE FROM "snap_4"."temporal_entity_type_members""#), "{evolving}");
}

#[test]
fn entries_are_numbered_from_the_base_ordinal() {
    for table in ["attribute_history", "scope_history"] {
        let sql = writing(table);
        assert!(sql.contains(r#""i"."base_ordinal" + ("e"."ordinality" - 1)"#), "{sql}");
    }
}

/// Clause 5.6.16 deletes a Temporal Evolution on its own, so the end of an
/// instance can reach an Entity that has none, and is recorded nowhere.
#[test]
fn a_deletion_of_an_entity_without_evolution_records_nothing() {
    for table in ["attribute_history", "scope_history", "temporal_attribute_streams"] {
        let sql = writing(table);
        assert!(sql.contains(r#""temporal_entities" AS "te""#), "{sql}");
        assert!(sql.contains(r#""te"."id" = "i"."entity_id""#), "{sql}");
    }
}

/// Clause 4.5.1 makes at least one type part of an Entity: an intent that
/// observes an Entity with no evolution and states none is refused by name.
#[test]
fn an_observation_without_evolution_or_types_is_named_by_the_pre_check() {
    let (role, sql) = rendered_steps()
        .into_iter()
        .find(|(role, _)| *role == StepRole::RefuseUntyped)
        .expect("a pre-check");
    assert_eq!(role, StepRole::RefuseUntyped);
    assert!(sql.starts_with(r#"SELECT "i"."intent_key", "i"."entity_id""#), "{sql}");
    assert!(sql.contains(r#"(NOT cardinality("i"."entity_type_name_keys") > 0)"#), "{sql}");
    assert!(
        sql.contains(r#""e"."entry_kind" <> 2"#),
        "an intent holding an entry that ends nothing observes: {sql}"
    );
    assert!(
        sql.contains(r#"(NOT EXISTS(SELECT 1 FROM "temporal_entities" AS "te" WHERE "te"."id" = "i"."entity_id"))"#),
        "{sql}"
    );
    let pre_check = position("SELECT");
    assert!(pre_check > position(r#"WITH "evolved""#), "the source's own evolutions count");
    assert!(
        pre_check < position(r#"INSERT INTO "temporal_attribute_streams""#),
        "an untyped intent's streams follow its refusal"
    );
}

/// An evolution the statement creates is invisible to every other writer
/// until the commit, so its streams are new and need no arbitration; the
/// streams of one that existed arbitrate on the grain.
#[test]
fn the_streams_of_an_evolution_the_statement_creates_are_inserted_without_arbitration() {
    let evolving = evolving();
    assert!(
        common_table(&evolving, "evolved").contains(r#"RETURNING "new"."temporal_entity_key", "new"."id", "old"."temporal_entity_key" IS NULL, "#),
        "{evolving}"
    );
    let created = common_table(&evolving, "created_streams");
    assert!(created.contains(r#"AS (INSERT INTO "temporal_attribute_streams""#), "{created}");
    assert!(
        created.ends_with(r#"FROM "grains" WHERE "grains"."created" ORDER BY "grains"."first_intent" ASC, "grains"."first_ordinal" ASC"#),
        "{created}"
    );
    assert!(!created.contains("ON CONFLICT"), "{created}");
    assert!(
        evolving.ends_with(concat!(
            r#"FROM "grains" WHERE NOT "grains"."created" ORDER BY "grains"."first_intent" ASC, "grains"."first_ordinal" ASC "#,
            r#"ON CONFLICT ("temporal_entity_key", "attribute_name_key", "dataset_id") DO NOTHING"#
        )),
        "{evolving}"
    );
}

/// The streams of the Entities intents state types for take the evolution's
/// key and identifier from what its insert returned, never from a read of
/// the evolutions,
/// so an evolution another writer committed while the statement ran still
/// gets its streams, and every intent of such an Entity is read.
#[test]
fn the_streams_of_a_typed_entity_take_the_key_its_evolution_insert_returned() {
    let evolving = evolving();
    let grains = common_table(&evolving, "grains");
    assert!(
        grains.contains(r#"SELECT "evolved"."temporal_entity_key", "evolved"."id", "e"."attribute_name_key", "e"."dataset_id", "evolved"."created""#),
        "{grains}"
    );
    assert!(
        grains.contains(r#"GROUP BY "evolved"."temporal_entity_key", "evolved"."id", "e"."attribute_name_key", "e"."dataset_id", "evolved"."created""#),
        "one row a grain: {grains}"
    );
    assert!(
        grains.contains(r#"WHERE "evolved"."id" = "i"."entity_id" AND "e"."entry_kind" IN (1, 2)"#),
        "{grains}"
    );
    assert!(!grains.contains("temporal_entities") && !grains.contains("cardinality("), "{grains}");
}

/// The streams are keyed in the order the history insert reaches their
/// entries, so the history rows arrive in ascending stream key and every
/// index led by it is extended at its right edge.
#[test]
fn the_streams_of_a_typed_entity_are_keyed_in_the_order_its_entries_are_recorded() {
    let evolving = evolving();
    let grains = common_table(&evolving, "grains");
    assert!(
        grains.contains(r#"MIN("i"."intent_key"), MIN("i"."base_ordinal" + ("e"."ordinality" - 1))"#),
        "{grains}"
    );
    assert_eq!(
        evolving
            .matches(r#"ORDER BY "grains"."first_intent" ASC, "grains"."first_ordinal" ASC"#)
            .count(),
        2,
        "{evolving}"
    );
}

/// The intents that state no type extend only an evolution that exists, so
/// only they reach the stream insert that reads the evolutions.
#[test]
fn only_an_intent_stating_no_type_reaches_the_untyped_stream_insert() {
    let streams = writing("temporal_attribute_streams");
    assert!(streams.contains(r#"(NOT cardinality("i"."entity_type_name_keys") > 0)"#), "{streams}");
    assert!(
        streams.contains(r#"("temporal_entity_key", "id", "attribute_name_key", "dataset_id") SELECT DISTINCT "te"."temporal_entity_key", "te"."id","#),
        "every stream carries its evolution's identifier: {streams}"
    );
    assert!(
        streams.ends_with(r#"ON CONFLICT ("temporal_entity_key", "attribute_name_key", "dataset_id") DO NOTHING"#),
        "{streams}"
    );
    let untyped = |entries: Vec<TemporalAppendEntry>| {
        let mut untyped = intent(entries);
        untyped.entity_types.clear();
        record_of(&untyped)
    };
    let reaches = |intents: &[TemporalIntentRecord]| needs_met(intents)[5];
    assert!(reaches(&[untyped(vec![tombstone()])]));
    assert!(!reaches(&[untyped(Vec::new())]));
    assert!(!reaches(&[untyped(vec![TemporalAppendEntry::Scope {
        observation: ScopeObservation::Deleted,
        kind: ScopeEntryKind::Event,
        observed_at: None,
    }])]));
    let typed = intent(vec![
        tombstone(),
        observation(TemporalValue::Number(JsonNumber::from(1)), InstanceIdentity::generate(), None),
    ]);
    assert!(!reaches(&[record_of(&typed)]));
}

/// Clause 4.5.7: a client-supplied `instanceId` names one instance across
/// every `datasetId` of the attribute, so the row it replaces is sought on
/// every sibling stream, by that identifier and never by its instant.
#[test]
fn a_replaced_instance_is_sought_across_every_dataset_of_its_attribute() {
    let replaced = in_role(StepRole::Replace);
    assert!(replaced.contains(r#""s2"."attribute_name_key" = "e"."attribute_name_key""#), "{replaced}");
    assert!(!replaced.contains(r#""s2"."dataset_id""#), "{replaced}");
    assert!(replaced.contains(r#""e"."verbatim_instance_id" IS NOT NULL"#), "{replaced}");
    assert!(!replaced.contains("observed_at") && !replaced.contains("generated_instance_id"), "{replaced}");
}

/// The replacement is read rather than deleted in place, so the delete that
/// follows can bind each replaced row's stream and write identity.
#[test]
fn a_replacement_reads_the_segment_and_write_identity_of_each_replaced_row() {
    let instances = in_role(StepRole::Replace);
    assert!(
        instances.starts_with(
            r#"SELECT "attribute_history"."stream_key", "attribute_history"."recorded_at", "attribute_history"."write_id", "attribute_history"."write_ordinal" FROM"#
        ),
        "{instances}"
    );
    assert!(!instances.contains("DELETE"), "{instances}");
}

/// Clause 4.5.6 gives the temporal Scope no `instanceId`, so a Scope row is
/// named by its write identity alone: no step reads Scope rows to replace
/// them, and an entry recorded once is absorbed by the insert's conflict
/// target.
#[test]
fn a_scope_row_is_recorded_under_its_write_identity_alone() {
    let scopes = writing("scope_history");
    assert!(
        scopes.contains(r#"ON CONFLICT ("recorded_at", "write_id", "write_ordinal") DO NOTHING"#),
        "{scopes}"
    );
    assert!(!scopes.contains("instance_id"), "{scopes}");
    assert!(
        rendered_steps()
            .iter()
            .filter(|(role, _)| *role == StepRole::Replace)
            .all(|(_, sql)| !sql.contains("scope_history"))
    );
}

/// The claims' primary key is what refuses two instances naming one
/// `instanceId`, so the claim insert has no conflict action to absorb one.
#[test]
fn a_duplicate_claim_fails_the_claim_insert() {
    let claims = writing("attribute_history_instance_claims");
    assert!(
        claims.contains(r#"("temporal_entity_key", "attribute_name_key", "verbatim_instance_id")"#),
        "{claims}"
    );
    assert!(!claims.contains("ON CONFLICT"), "{claims}");
}

/// The prefix of the extent growth, in the live branch.
const GROWTH: &str = r#"UPDATE "temporal_attribute_streams" SET "value_extent" = "#;

/// The extent growth of the drain.
fn growth() -> String {
    rendered_steps()
        .into_iter()
        .map(|(_, sql)| sql)
        .find(|sql| sql.starts_with(GROWTH))
        .expect("an extent growth")
}

#[test]
fn an_application_grows_the_extent_after_recording_the_history() {
    let history = position(r#"INSERT INTO "attribute_history""#);
    assert!(history < position(GROWTH), "{:?}", rendered_steps());
}

#[test]
fn the_extent_grows_from_the_geometry_the_history_row_stores() {
    let sql = growth();
    assert!(
        sql.starts_with(concat!(
            r#"UPDATE "temporal_attribute_streams" SET "value_extent" = "#,
            r#"ST_Envelope(ST_Collect("temporal_attribute_streams"."value_extent", "grown"."extent")) FROM (SELECT "s"."stream_key", "#,
            r#"ST_Envelope(ST_Collect(ST_MakeValid("e"."value_geometry"))) AS "extent" FROM "#
        )),
        "{sql}"
    );
    assert!(writing("attribute_history").contains(r#"ST_MakeValid("e"."value_geometry")"#));
    assert!(sql.contains(r#" GROUP BY "s"."stream_key") AS "grown""#), "{sql}");
    assert!(sql.contains(r#""temporal_attribute_streams"."stream_key" = "grown"."stream_key""#), "{sql}");
}

#[test]
fn an_extent_covering_the_write_is_left_alone() {
    let sql = growth();
    assert!(
        sql.ends_with(concat!(
            r#"AND ("temporal_attribute_streams"."value_extent" IS NULL"#,
            r#" OR (NOT ("temporal_attribute_streams"."value_extent" ~ "grown"."extent")))"#
        )),
        "{sql}"
    );
}

/// A tombstone records no value, and a value of another kind no geometry, so
/// neither has a box to grow an extent by.
#[test]
fn a_tombstone_or_a_non_geometry_entry_never_grows_an_extent() {
    let sql = growth();
    let grouped = &sql[sql.find(r"FROM (SELECT").expect("the grouped entries")..sql.find(r#") AS "grown""#).expect("the grouped entries end")];
    assert!(grouped.contains(r#"AND "e"."value_geometry" IS NOT NULL GROUP BY"#), "{grouped}");
    assert!(grouped.contains(r#""e"."entry_kind" IN (1, 2)"#), "{grouped}");
}

#[test]
fn a_snapshot_application_grows_its_own_streams() {
    let schema = Alias::new("snap_4").into_iden();
    let steps = steps_over(
        &IntentSource::Queued {
            keys: &[3, 5],
            reach: TemporalIntentReach::ALL.to_vec(),
        },
        Some(&schema),
    );
    let growth = steps
        .into_iter()
        .map(|(_, sql)| sql)
        .find(|sql| sql.starts_with(r#"UPDATE "snap_4"."temporal_attribute_streams" SET "value_extent" = "#))
        .expect("an extent growth in the Snapshot");
    assert!(growth.contains(r#""snap_4"."temporal_attribute_streams" AS "s""#), "{growth}");
    assert!(!growth.contains(r#"FROM "temporal_attribute_streams""#), "{growth}");
}

#[test]
fn the_rows_land_once_under_their_write_identity() {
    for table in ["attribute_history", "scope_history"] {
        let sql = writing(table);
        assert!(sql.contains(r#"ON CONFLICT ("recorded_at", "write_id", "write_ordinal") DO NOTHING"#), "{sql}");
    }
    let history = writing("attribute_history");
    assert!(history.contains(r#"ST_MakeValid("e"."value_geometry")"#), "{history}");
    assert!(writing("scope_history").contains(r#""e"."scope_path"::ltree"#));
}
