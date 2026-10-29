use crate::{
    intent_application::{
        StepRole,
        StepStatement,
        application_steps,
        intent_source::{EntryReads, IntentSource, StagedSource, reach_statement},
    },
    temporal_entry_record::{
        TemporalIntentRecord,
        tests::{intent, keys, observation, recorded_at},
    },
};
use andromeda_context_commit::{write_id::WriteId, write_record::WriteRecord};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier, instance_identity::InstanceIdentity, scope::path::ScopePath};
use andromeda_ngsi_ld_model::temporal::TemporalValue;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    scope_entry_kind::ScopeEntryKind,
    scope_observation::ScopeObservation,
};
use pretty_assertions::assert_eq;
use sea_query::{Alias, DynIden, Expr, IntoIden, PostgresQueryBuilder, Query};
use std::{collections::BTreeSet, sync::LazyLock};

/// The queue rows the drain reads, as every statement names them.
const QUEUED: &str = r#"(SELECT * FROM "temporal_append_intents" WHERE "intent_key" = ANY(ARRAY [3,5]::bigint[])) AS "i""#;

/// The write record every commit statement is built over, so the text of the
/// bound source is the same in every statement.
static COMMIT_RECORD: LazyLock<WriteRecord> = LazyLock::new(|| WriteRecord::restored(WriteId::mint(), recorded_at()));

/// The text a value renders to inline.
fn inlined(value: sea_query::SimpleExpr) -> String {
    Query::select()
        .expr(value)
        .to_owned()
        .to_string(PostgresQueryBuilder)
        .trim_start_matches("SELECT ")
        .to_owned()
}

/// The intents a commit binds, as every statement names them: the intents
/// travel as the one external argument, which renders as its marker.
fn bound() -> String {
    let write_id = inlined(Expr::val(COMMIT_RECORD.write_id().as_uuid()));
    let recorded_at = inlined(Expr::val(COMMIT_RECORD.recorded_at()));
    format!(
        concat!(
            r#"(SELECT {write_id}::uuid AS "write_id", {recorded_at}::timestamptz AS "recorded_at", "input"."entity_id", "#,
            r#""input"."entity_type_name_keys", "input"."base_ordinal", "input"."entries", "input"."ordinality" AS "intent_key" "#,
            r#"FROM UNNEST({intents}::temporal_intent[]) AS "input") AS "i""#,
        ),
        write_id = write_id,
        recorded_at = recorded_at,
        intents = "\u{0}0\u{0}",
    )
}

/// The keys of the evolutions the statements are built over.
const CREATED: [i64; 2] = [7, 9];

/// The keys of the evolutions the application created, as a statement names
/// them.
const CREATED_ARRAY: &str = "ARRAY [7,9]::bigint[]";

/// A statement as the text sea-query renders it, its values inline.
fn text_of(statement: &StepStatement) -> String {
    match statement {
        StepStatement::Select(statement) => statement.to_string(PostgresQueryBuilder),
        StepStatement::Insert(statement) => statement.to_string(PostgresQueryBuilder),
        StepStatement::Delete(statement) => statement.to_string(PostgresQueryBuilder),
        StepStatement::Update(statement) => statement.to_string(PostgresQueryBuilder),
        StepStatement::With(statement) => statement.to_string(PostgresQueryBuilder),
    }
}

/// The statements of an application over `source` in the branch `schema`, in
/// order, built over the created keys [`CREATED`].
fn steps_over(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Vec<(StepRole, String)> {
    steps_with_reads(source, schema).into_iter().map(|(role, _, sql)| (role, sql)).collect()
}

/// The statements of an application over `source` in the branch `schema`, in
/// order, each with the entries it reads.
fn steps_with_reads(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Vec<(StepRole, EntryReads, String)> {
    application_steps()
        .into_iter()
        .map(|step| {
            let staged = StagedSource::new(source, step.reads, &CREATED);
            let statement = (step.build)(&staged, schema).expect("every statement builds");
            (step.role, step.reads, text_of(&statement))
        })
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
    steps_over(
        &IntentSource::Bound {
            record: &COMMIT_RECORD,
            intents: &[],
            established: &[],
        },
        None,
    )
}

/// The statement among `steps` that writes into `table`, a stream insert
/// counting its streams into the census from the common table it runs in.
fn writing_in(steps: Vec<(StepRole, String)>, table: &str) -> String {
    let prefix = format!(r#"INSERT INTO "{table}""#);
    let counted = format!("{COUNTED_STREAMS}{prefix}");
    steps
        .into_iter()
        .map(|(_, sql)| sql)
        .find(|sql| sql.starts_with(&prefix) || sql.starts_with(&counted))
        .unwrap_or_else(|| panic!("no statement writes {table}"))
}

/// The head of a statement whose stream change the census counts.
const COUNTED_STREAMS: &str = r#"WITH "changed_streams" AS ("#;

/// The head of the stream insert of the intents stating no type.
const UNTYPED_STREAMS: &str = r#"WITH "changed_streams" AS (INSERT INTO "temporal_attribute_streams""#;

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

/// The types an evolution gained, as the evolution statement reads them.
const GAINED_TYPES: &str = r#"ARRAY (SELECT "gt" FROM UNNEST("evolved"."new_types") AS "gt" WHERE NOT "gt" = ANY("evolved"."old_types"))"#;

/// The head of the query of the evolution statement, which returns the
/// evolutions it created or gave a type.
const EVOLVED_ROWS: &str = r#") SELECT "evolved"."entity_key", "evolved"."created", "#;

/// The query `sql` names `name` in its `WITH` clause, up to the query that
/// follows it.
fn common_table<'s>(sql: &'s str, name: &str) -> &'s str {
    let start = sql.find(&format!(r#""{name}" "#)).unwrap_or_else(|| panic!("no {name} in {sql}"));
    let rest = &sql[start..];
    let end = [") , ", ") INSERT ", EVOLVED_ROWS]
        .iter()
        .filter_map(|follower| rest.find(follower))
        .min()
        .unwrap_or_else(|| panic!("{name} ends nowhere in {sql}"));
    &rest[..end]
}

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

/// The position of the first statement whose text starts with `prefix`.
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
    let source = IntentSource::Bound {
        record: &record,
        intents,
        established: &[],
    };
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
    assert!(history.contains(&format!(r#"FROM {QUEUED}, UNNEST("i"."entries") AS "e""#)), "{history}");
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

/// The source relation `relation` names under `i`, as the evolution upsert
/// reads it again under `si` for the types its intents state.
fn stating(relation: &str) -> String {
    relation.replace(r#"AS "i""#, r#"AS "si""#)
}

/// The source relation `relation` names under `i`, as the deletion judgement
/// reads it again under `ai` for an intent stating types at or before one.
fn anchoring(relation: &str) -> String {
    relation.replace(r#"AS "i""#, r#"AS "ai""#)
}

#[test]
fn the_commit_and_the_drain_render_one_statement_shape() {
    let drain: Vec<(StepRole, String)> = rendered_steps()
        .into_iter()
        .map(|(role, sql)| {
            (
                role,
                sql.replace(QUEUED, "<source>")
                    .replace(&earlier(QUEUED), "<source>")
                    .replace(&stating(QUEUED), "<source>")
                    .replace(&anchoring(QUEUED), "<source>"),
            )
        })
        .collect();
    let bound = bound();
    let commit: Vec<(StepRole, String)> = commit_steps()
        .into_iter()
        .map(|(role, sql)| {
            (
                role,
                sql.replace(&bound, "<source>")
                    .replace(&earlier(&bound), "<source>")
                    .replace(&stating(&bound), "<source>")
                    .replace(&anchoring(&bound), "<source>"),
            )
        })
        .collect();
    assert_eq!(drain, commit);
    assert!(drain.iter().all(|(_, sql)| sql.contains("<source>")), "{drain:?}");
}

#[test]
fn a_commit_binds_its_intents_as_one_typed_array() {
    let bound = bound();
    for (_, sql) in commit_steps() {
        assert!(sql.contains(&bound), "{sql}");
        assert_eq!(
            sql.matches("::temporal_intent[]").count(),
            sql.matches(&bound).count()
                + sql.matches(&earlier(&bound)).count()
                + sql.matches(&stating(&bound)).count()
                + sql.matches(&anchoring(&bound)).count(),
            "{sql}"
        );
        assert!(!sql.contains("temporal_append_intents"), "{sql}");
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
        sql.contains(r#"FROM "scope_history" AS "sr" WHERE "sr"."entity_key" = "te"."entity_key""#),
        "{sql}"
    );
    assert!(
        sql.contains(r#"FROM UNNEST("i"."entries") AS "sa" WHERE "sa"."entry_kind" = 4 ORDER BY ("sa"."verbatim_scope_path") COLLATE "C" ASC"#),
        "{sql}"
    );
    assert_eq!(sql.matches(r#"COLLATE "C") = ((ARRAY ("#).count(), 2, "{sql}");
}

/// A change of Scope, a reified instance and a removal are events whatever
/// the history holds: only the assertion kind is judged.
#[test]
fn a_change_an_instance_and_a_removal_are_inserted_unconditionally() {
    let sql = scope_insert();
    assert!(sql.contains(r#""e"."entry_kind" IN (3, 4)"#), "{sql}");
    assert!(sql.contains(r#"("e"."entry_kind" <> 4 OR "#), "{sql}");
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
    let union = r#"FROM "scope_history" AS "sr" WHERE "sr"."entity_key" = "te"."entity_key" UNION ALL"#;
    assert_eq!(sql.matches(union).count(), 4, "two reads of each of two axes: {sql}");
}

/// No application runs while an earlier intent of one of its Entities is
/// queued outside its source, so every entry an assertion is judged against is
/// recorded or in the source, and no source reads the queue to judge one.
#[test]
fn an_assertion_is_judged_without_reading_the_queue_in_any_source() {
    let drain = scope_insert();
    assert!(!drain.contains(r#""temporal_append_intents" AS "sq""#), "{drain}");
    let commit = writing_in(commit_steps(), "scope_history");
    assert!(!commit.contains("temporal_append_intents"), "{commit}");
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    let schema = Alias::new("snap_1").into_iden();
    let snapshot = writing_in(
        steps_over(
            &IntentSource::Bound {
                record: &record,
                intents: &[],
                established: &[],
            },
            Some(&schema),
        ),
        r#"snap_1"."scope_history"#,
    );
    assert!(!snapshot.contains("temporal_append_intents"), "{snapshot}");
}

/// Clause 5.6.16 deletes a Temporal Evolution on its own, so the end of an
/// instance a commit records can reach an Entity that has none, and is
/// recorded nowhere: the drain's join decides it.
#[test]
fn a_native_deletion_of_an_entity_without_evolution_records_nothing() {
    for table in ["attribute_history", "scope_history", "temporal_attribute_streams"] {
        let sql = writing_in(commit_steps(), table);
        assert!(sql.contains(r#""entities" AS "te""#), "{sql}");
        assert!(sql.contains(r#""te"."id" = "i"."entity_id""#), "{sql}");
    }
}

/// A Snapshot branch records its history in its own relations; the queue
/// belongs to the tenant.
#[test]
fn a_commit_to_a_snapshot_branch_records_in_its_schema() {
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    let schema = Alias::new("snap_7").into_iden();
    let steps = steps_over(
        &IntentSource::Bound {
            record: &record,
            intents: &[],
            established: &[],
        },
        Some(&schema),
    );
    for table in [
        "entities",
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
}

/// A statement no entry of a commit reaches is not run: a typed observation
/// with a broker-minted instance needs no claim and no Scope row.
#[test]
fn a_commit_runs_only_the_statements_its_entries_reach() {
    let typed = record_of(&intent(vec![observation(
        TemporalValue::Number {
            number: JsonNumber::from(42),
            datatype: None,
        },
        InstanceIdentity::generate(),
        None,
    )]));
    assert_eq!(needs_met(&[typed]), [true, false, true, false, false, false]);

    let claimed = record_of(&intent(vec![observation(
        TemporalValue::Number {
            number: JsonNumber::from(42),
            datatype: None,
        },
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
        observation(
            TemporalValue::Number {
                number: JsonNumber::from(1),
                datatype: None,
            },
            InstanceIdentity::generate(),
            None,
        ),
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
    let sql = reach_statement(&[3, 5]).to_string(PostgresQueryBuilder);
    assert_eq!(
        sql,
        r#"SELECT DISTINCT UNNEST("entry_needs") FROM "temporal_append_intents" WHERE "intent_key" = ANY(ARRAY [3,5]::bigint[])"#
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
    let source = IntentSource::Bound {
        record: &COMMIT_RECORD,
        intents: &[],
        established: &[],
    };
    for (_, reads, sql) in steps_with_reads(&source, None) {
        let read = entry_fields_read(&sql);
        match reads {
            EntryReads::Identities => assert!(read.is_subset(&identity), "{read:?} in {sql}"),
            EntryReads::Everything => assert!(!read.is_empty(), "{sql}"),
        }
    }
}

/// The light form a bound source sends keeps what the statements reading it
/// read, and drops every value.
#[test]
fn a_bound_identity_drops_every_value() {
    let full = record_of(&intent(vec![observation(
        TemporalValue::Number {
            number: JsonNumber::from(42),
            datatype: None,
        },
        InstanceIdentity::Verbatim("urn:x".parse().expect("an instance identifier")),
        None,
    )]));
    let identities = full.identities();
    assert_eq!(
        (&identities.entity_id, &identities.entity_type_name_keys, identities.base_ordinal),
        (&full.entity_id, &full.entity_type_name_keys, full.base_ordinal)
    );
    assert_eq!(
        identities.entry_needs,
        [
            TemporalIntentReach::TypedIntent,
            TemporalIntentReach::AttributeEntry,
            TemporalIntentReach::VerbatimInstance
        ]
        .map(TemporalIntentReach::code)
    );
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
    let untyped_streams = position(UNTYPED_STREAMS);
    let history = position(r#"INSERT INTO "attribute_history""#);
    let scopes = position(r#"INSERT INTO "scope_history""#);
    assert!(
        evolutions < untyped_streams && untyped_streams < history && evolutions < scopes,
        "{:?}",
        rendered_steps()
    );
    assert!(role_position(StepRole::Replace) < history, "the replaced instance goes first");
    assert!(position(r#"INSERT INTO "attribute_history_instance_claims""#) < history);
}

/// Clause 5.7.4.4 decides eligibility from every family and shape a stream
/// holds, so the application grows each stream's mask by the combination of
/// each recorded instance, as the aggregation projection refines it, once the
/// history rows exist; a tombstone carries no value and adds none, and a mask
/// already holding every combination is not rewritten.
#[test]
fn the_shapes_of_the_recorded_instances_are_added_to_their_streams() {
    let shapes = shape_growth();
    assert!(position(r#"INSERT INTO "attribute_history""#) < position(SHAPE_GROWTH));
    assert!(
        shapes.contains(r#""s"."stream_key", "e"."#) && shapes.contains(r#") AS "ah" WHERE "ah"."value_kind" <> (0) GROUP BY "ah"."stream_key") AS "grown""#),
        "{shapes}"
    );
    assert!(
        shapes.contains(r#"bit_or((B'1'::bit(80) >> ((("ah"."attribute_kind" - 1) * (10)) + ((CASE"#),
        "{shapes}"
    );
    assert!(
        shapes.ends_with(r#"("temporal_attribute_streams"."shape_mask" | "grown"."shape_mask") <> "temporal_attribute_streams"."shape_mask""#),
        "{shapes}"
    );
}

/// A source may hold several intents of one Entity, and a conflict action
/// changes a row once per statement, so the evolution is written in one row
/// carrying every type the Entity's intents state, at the instant of its
/// latest typed intent.
#[test]
fn one_row_of_an_entity_carries_every_type_its_intents_state() {
    let evolving = evolving();
    let evolutions = common_table(&evolving, "evolved");
    assert!(evolutions.contains(r#"AS (INSERT INTO "entities""#), "{evolutions}");
    assert!(evolutions.contains(r#"SELECT DISTINCT ON ("i"."entity_id")"#), "{evolutions}");
    assert!(evolutions.contains(r#"ORDER BY "i"."entity_id" ASC, "i"."intent_key" DESC"#), "{evolutions}");
    assert!(evolutions.contains(r#"cardinality("i"."entity_type_name_keys") > 0"#), "{evolutions}");
    assert!(
        evolutions.contains(r#""i"."recorded_at", "stated_types"."entity_type_name_keys" FROM"#),
        "{evolutions}"
    );
    assert!(
        evolutions
            .contains(r#"(SELECT "si"."entity_id" AS "id", "si"."entity_type_name_keys" AS "entity_type_name_keys", "si"."intent_key" AS "list_rank" FROM"#),
        "{evolutions}"
    );
    assert!(
        evolutions.contains(r#"AS "stated_types" WHERE "stated_types"."id" = "i"."entity_id""#),
        "{evolutions}"
    );
}

/// Clauses 5.6.11.4 and 5.6.12.4 add the types a write states that the
/// evolution lacks to its list, and the conflict action reads the list as the
/// last committed writer left it, so a concurrent writer's types are kept.
#[test]
fn an_existing_evolution_keeps_its_types_and_gains_the_ones_it_lacks() {
    let evolving = evolving();
    let evolutions = common_table(&evolving, "evolved");
    assert!(
        evolutions.contains(concat!(
            r#""evolution_type_name_keys" = array_cat(COALESCE("entities"."evolution_type_name_keys", '{}'::integer[]), "#,
            r#"ARRAY (SELECT "added"."added" FROM UNNEST("excluded"."evolution_type_name_keys") AS "added" "#,
            r#"WHERE "added"."added" <> ALL(COALESCE("entities"."evolution_type_name_keys", '{}'::integer[])) "#,
            r#"ORDER BY array_position("excluded"."evolution_type_name_keys", "added"."added") ASC))"#
        )),
        "{evolutions}"
    );
}

/// Clause 4.8 makes `modifiedAt` the instant of the last modification, so an
/// application stamped before the stored evolution leaves it where it is, and
/// `createdAt` the instant the Evolution began, which an identity row holding
/// a current state alone takes from the application.
#[test]
fn an_evolutions_modification_instant_never_moves_back() {
    let evolving = evolving();
    let evolutions = common_table(&evolving, "evolved");
    assert!(
        evolutions.contains(concat!(
            r#"DO UPDATE SET "evolution_created_at" = COALESCE("entities"."evolution_created_at", "excluded"."evolution_created_at"), "#,
            r#""evolution_modified_at" = GREATEST("entities"."evolution_modified_at", "excluded"."evolution_modified_at")"#
        )),
        "{evolutions}"
    );
    assert!(!evolutions.contains(r#" "created_at" ="#), "the current state is left alone: {evolutions}");
}

/// The membership moves read the types the evolution had and has from the
/// upsert's own images: the replaced row is the one it found after waiting on
/// a concurrent writer, which the statement's snapshot may predate.
#[test]
fn an_evolution_returns_the_types_it_had_and_has() {
    let evolving = evolving();
    let evolutions = common_table(&evolving, "evolved");
    assert!(
        evolutions.starts_with(r#""evolved" ("entity_key", "id", "created", "new_types", "old_types") AS"#),
        "{evolutions}"
    );
    assert!(
        evolutions.ends_with(r#"RETURNING "new"."entity_key", "new"."id", "old"."evolution_created_at" IS NULL, "new"."evolution_type_name_keys", "old"."evolution_type_name_keys""#),
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
        added.contains(r#"AS (INSERT INTO "entity_type_members" ("entity_key", "entity_type_name_key", "id", "is_current_member", "is_evolution_member") SELECT "evolved"."entity_key", "carried"."entity_type_name_key", "evolved"."id", FALSE, TRUE FROM "evolved""#),
        "{added}"
    );
    assert!(added.contains(r#"UNNEST("evolved"."new_types")"#), "{added}");
    assert!(
        added.contains(r#"WHERE "evolved"."created" OR (NOT "carried"."entity_type_name_key" = ANY("evolved"."old_types"))"#),
        "{added}"
    );
}

/// No operation removes a type from an evolution, so the statement deletes no
/// membership row: one it could not see in its snapshot, a concurrent
/// writer's, is never left without its type.
#[test]
fn an_evolution_never_drops_a_membership_row() {
    let evolving = evolving();
    assert!(!evolving.contains("dropped_members"), "{evolving}");
    assert!(!evolving.contains(r#"DELETE FROM "entity_type_members""#), "{evolving}");
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
    assert!(evolving.contains(r#"INSERT INTO "snap_4"."entity_type_members""#), "{evolving}");
}

/// The Entity a commit's create established the Temporal Evolution of.
fn established_vehicle() -> Identifier {
    "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")
}

/// The evolution statement of a commit whose create established the
/// evolution of [`established_vehicle`].
fn established_evolving() -> String {
    let established = [established_vehicle()];
    steps_over(
        &IntentSource::Bound {
            record: &COMMIT_RECORD,
            intents: &[],
            established: &established,
        },
        None,
    )
    .into_iter()
    .map(|(_, sql)| sql)
    .find(|sql| sql.starts_with(r#"WITH "upserted""#))
    .expect("an evolution statement over the upsert")
}

/// The established identifiers, as the statement names them.
const ESTABLISHED_ARRAY: &str = r"IN (SELECT UNNEST(ARRAY ['urn:ngsi-ld:Vehicle:1']::text[]))";

/// A drain and a commit establishing nothing keep the one plain statement,
/// so only a commit whose create wrote an evolution runs the other shape.
#[test]
fn a_source_establishing_no_evolution_keeps_the_plain_evolution_statement() {
    for steps in [rendered_steps(), commit_steps()] {
        let evolving = steps
            .into_iter()
            .map(|(_, sql)| sql)
            .find(|sql| sql.contains(r#"INSERT INTO "entities""#))
            .expect("an evolution statement");
        assert!(evolving.starts_with(r#"WITH "evolved""#), "{evolving}");
        assert!(!evolving.contains("upserted"), "{evolving}");
        assert!(!evolving.contains(r#""est""#), "{evolving}");
    }
}

/// The create wrote the evolution an identity row holding none is given, so
/// the conflict action proposes the inserted values as they are and writes
/// nothing where the row already holds them: the create's row stays the only
/// version.
#[test]
fn an_established_evolution_is_left_where_its_row_holds_what_the_intents_establish() {
    let evolving = established_evolving();
    let upserted = common_table(&evolving, "upserted");
    let established = format!(r#""entities"."id" {ESTABLISHED_ARRAY}"#);
    assert!(
        upserted.contains(&format!(
            r#"DO UPDATE SET "evolution_created_at" = (CASE WHEN ({established}) THEN "excluded"."evolution_created_at" ELSE COALESCE("entities"."evolution_created_at", "excluded"."evolution_created_at") END)"#
        )),
        "{upserted}"
    );
    assert!(
        upserted.contains(&format!(
            r#""evolution_modified_at" = (CASE WHEN ({established}) THEN "excluded"."evolution_modified_at" ELSE GREATEST("entities"."evolution_modified_at", "excluded"."evolution_modified_at") END)"#
        )),
        "{upserted}"
    );
    assert!(
        upserted.contains(&format!(
            r#"WHERE NOT ({established} AND "entities"."evolution_created_at" = "excluded"."evolution_created_at" AND "entities"."evolution_modified_at" = "excluded"."evolution_modified_at" AND "entities"."evolution_type_name_keys" = "excluded"."evolution_type_name_keys")"#
        )),
        "{upserted}"
    );
}

/// An established evolution the upsert rewrites is still one the statement
/// created, with no types before, as an identity row holding none returns.
#[test]
fn a_rewritten_established_evolution_is_returned_as_created() {
    let evolving = established_evolving();
    let upserted = common_table(&evolving, "upserted");
    let established = format!(r#""new"."id" {ESTABLISHED_ARRAY}"#);
    assert!(
        upserted.ends_with(&format!(
            r#"RETURNING "new"."entity_key", "new"."id", "old"."evolution_created_at" IS NULL OR {established}, "new"."evolution_type_name_keys", (CASE WHEN ({established}) THEN NULL::integer[] ELSE "old"."evolution_type_name_keys" END)"#
        )),
        "{upserted}"
    );
}

/// An established evolution the upsert left joins `evolved` from the
/// identity row as the create wrote it, as created, so its streams are
/// inserted without arbitration and counted under its types, but only where
/// the source states types for it and the upsert did not return it already.
#[test]
fn an_established_evolution_the_upsert_left_is_evolved_as_created() {
    let evolving = established_evolving();
    assert!(
        evolving.contains(concat!(
            r#""evolved" ("entity_key", "id", "created", "new_types", "old_types") AS (SELECT "upserted"."entity_key", "upserted"."id", "upserted"."created", "upserted"."new_types", "upserted"."old_types" FROM "upserted" "#,
            r#"UNION ALL (SELECT "est"."entity_key", "est"."id", TRUE, "est"."evolution_type_name_keys", NULL::integer[] FROM "entities" AS "est" "#
        )),
        "{evolving}"
    );
    assert!(
        evolving.contains(&format!(
            r#"WHERE "est"."id" {ESTABLISHED_ARRAY} AND "est"."id" IN (SELECT "ei"."entity_id" FROM "#
        )),
        "{evolving}"
    );
    assert!(evolving.contains(r#"WHERE cardinality("ei"."entity_type_name_keys") > 0)"#), "{evolving}");
    assert!(
        evolving.contains(r#"AND (NOT "est"."id" IN (SELECT "upserted"."id" FROM "upserted")))"#),
        "{evolving}"
    );
}

/// The create wrote an established evolution's memberships on both sides,
/// so the memberships are moved from what the upsert wrote alone.
#[test]
fn an_established_evolution_lands_no_second_membership_row() {
    let evolving = established_evolving();
    let added = common_table(&evolving, "added_members");
    assert!(
        added.contains(r#"SELECT "upserted"."entity_key", "carried"."entity_type_name_key", "upserted"."id", FALSE, TRUE FROM "upserted""#),
        "{added}"
    );
    assert!(!added.contains(r#"FROM "evolved""#), "{added}");
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
        assert!(sql.contains(r#""entities" AS "te""#), "{sql}");
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
        sql.contains(r#""e"."entry_kind" <> (2)"#),
        "an intent holding an entry that ends nothing observes: {sql}"
    );
    assert!(
        sql.contains(r#"(NOT EXISTS(SELECT 1 FROM "entities" AS "te" WHERE "te"."id" = "i"."entity_id" AND "te"."evolution_created_at" IS NOT NULL))"#),
        "{sql}"
    );
    let pre_check = position("SELECT");
    assert!(pre_check > position(r#"WITH "evolved""#), "the source's own evolutions count");
    assert!(pre_check < position(UNTYPED_STREAMS), "an untyped intent's streams follow its refusal");
}

/// An evolution the statement creates is invisible to every other writer
/// until the commit, so its streams are new and need no arbitration; the
/// streams of one that existed arbitrate on the grain.
#[test]
fn the_streams_of_an_evolution_the_statement_creates_are_inserted_without_arbitration() {
    let evolving = evolving();
    assert!(
        common_table(&evolving, "evolved").contains(r#"RETURNING "new"."entity_key", "new"."id", "old"."evolution_created_at" IS NULL, "#),
        "{evolving}"
    );
    let created = common_table(&evolving, "created_streams");
    assert!(created.contains(r#"AS (INSERT INTO "temporal_attribute_streams""#), "{created}");
    assert!(
        created.ends_with(concat!(
            r#"FROM "grains" WHERE "grains"."created" ORDER BY "grains"."first_intent" ASC, "grains"."first_ordinal" ASC "#,
            r#"RETURNING "entity_key", "attribute_name_key""#
        )),
        "{created}"
    );
    assert!(!created.contains("ON CONFLICT"), "{created}");
    let existing = common_table(&evolving, "existing_streams");
    assert!(existing.contains(r#"AS (INSERT INTO "temporal_attribute_streams""#), "{existing}");
    assert!(
        existing.ends_with(concat!(
            r#"FROM "grains" WHERE NOT "grains"."created" ORDER BY "grains"."first_intent" ASC, "grains"."first_ordinal" ASC "#,
            r#"ON CONFLICT ("entity_key", "attribute_name_key", COALESCE("dataset_id", '')) DO NOTHING RETURNING "entity_key", "attribute_name_key""#
        )),
        "{existing}"
    );
}

/// A stream the evolution statement inserts counts under every type of an
/// evolution it created, and under the types an existing evolution held
/// before: the types that one gained are counted afterwards over all its
/// streams, so no stream it held before is read in the statement's snapshot.
#[test]
fn the_streams_the_evolution_statement_inserts_count_under_the_types_held_before_the_gain() {
    let evolving = evolving();
    let census = common_table(&evolving, "stream_census");
    assert!(
        census.contains(concat!(
            r#"FROM "created_streams" JOIN "evolved" ON "evolved"."entity_key" = "created_streams"."entity_key" "#,
            r#"INNER JOIN LATERAL (SELECT UNNEST("evolved"."new_types")"#
        )),
        "{census}"
    );
    assert!(
        census.contains(concat!(
            r#"FROM "existing_streams" JOIN "evolved" ON "evolved"."entity_key" = "existing_streams"."entity_key" "#,
            r#"INNER JOIN LATERAL (SELECT UNNEST("evolved"."old_types")"#
        )),
        "{census}"
    );
    assert!(!census.contains(r#""temporal_attribute_streams""#), "{census}");
}

/// The statements after the evolution upsert judge the ends of an instance
/// by the evolutions it created, and the stream census counts the streams of
/// the types an existing one gained, so it returns those evolutions alone.
#[test]
fn the_evolution_statement_returns_the_evolutions_it_created_or_gave_a_type() {
    let evolving = evolving();
    assert!(
        evolving.ends_with(&format!(
            r#"{EVOLVED_ROWS}{GAINED_TYPES} FROM "evolved" WHERE "evolved"."created" OR cardinality({GAINED_TYPES}) > 0"#
        )),
        "{evolving}"
    );
    assert_eq!(
        rendered_steps().iter().filter(|(role, _)| *role == StepRole::Evolve).count(),
        1,
        "one statement creates evolutions"
    );
}

/// The judgement shared by every statement reading the attribute entries of
/// an evolution, its created keys bound after the drain's claimed keys.
const JUDGED: &str = concat!(
    r#"(NOT ("e"."entry_kind" = 2 AND "te"."entity_key" = ANY(ARRAY [7,9]::bigint[]) AND (NOT EXISTS(SELECT 1 FROM "#,
    r#"(SELECT * FROM "temporal_append_intents" WHERE "intent_key" = ANY(ARRAY [3,5]::bigint[])) AS "ai" "#,
    r#"WHERE "ai"."entity_id" = "i"."entity_id" AND cardinality("ai"."entity_type_name_keys") > 0 "#,
    r#"AND "ai"."intent_key" <= "i"."intent_key"))))"#
);

/// Applied on its own, the end of an instance finds the evolution its Entity
/// held before the batch or one an intent stating types at or before its own
/// created (clauses 4.5.6 and 5.6.16), so the batch drops the end of an
/// instance of an evolution it created before the first intent of its Entity
/// stating types, as a temporal read of the queue does: it is neither
/// recorded, nor replaces or claims an instance, nor extends a stream.
#[test]
fn every_statement_reading_attribute_entries_judges_the_ends_of_an_instance_by_the_evolutions_created() {
    for sql in [
        writing("attribute_history"),
        writing("attribute_history_instance_claims"),
        shape_growth(),
        writing("temporal_attribute_streams"),
        in_role(StepRole::Replace),
        growth(),
    ] {
        assert!(sql.contains(JUDGED), "{sql}");
    }
    let released = rendered_steps()
        .into_iter()
        .map(|(_, sql)| sql)
        .find(|sql| sql.starts_with(r#"DELETE FROM "attribute_history_instance_claims""#))
        .expect("a claim release");
    assert!(released.contains(JUDGED), "{released}");
}

/// A stream of an evolution the statement created is not given to an end of
/// an instance coming before every intent of its Entity stating types, while
/// one of an evolution that existed is, as the read shows it.
#[test]
fn the_grains_of_a_created_evolution_leave_out_the_ends_of_an_instance_before_its_first_typed_intent() {
    let evolving = evolving();
    let grains = common_table(&evolving, "grains");
    assert!(
        grains.contains(r#"AND (NOT ("e"."entry_kind" = 2 AND "evolved"."created" AND (NOT EXISTS(SELECT 1 FROM "#),
        "{grains}"
    );
    assert!(grains.contains(r#""ai"."intent_key" <= "i"."intent_key""#), "{grains}");
}

/// A Scope entry needs an evolution but no order among the intents, as the
/// read judges it, so the Scope insert reads no created key.
#[test]
fn a_scope_entry_is_not_judged_by_the_evolutions_created() {
    let scopes = scope_insert();
    assert!(!scopes.contains(CREATED_ARRAY) && !scopes.contains(r#"AS "ai""#), "{scopes}");
}

/// Only the statements that decide the end of an instance by the evolutions the
/// application created name their keys, in the drain and in a commit alike.
#[test]
fn only_the_statements_after_the_evolutions_name_the_created_keys() {
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    let sources = [
        IntentSource::Queued {
            keys: &[3, 5],
            reach: TemporalIntentReach::ALL.to_vec(),
        },
        IntentSource::Bound {
            record: &record,
            intents: &[],
            established: &[],
        },
    ];
    for source in &sources {
        let naming: Vec<(StepRole, bool)> = steps_over(source, None)
            .into_iter()
            .map(|(role, sql)| (role, sql.contains(CREATED_ARRAY)))
            .collect();
        assert_eq!(
            naming,
            [
                (StepRole::Evolve, false),
                (StepRole::RefuseUntyped, false),
                (StepRole::Write, true),
                (StepRole::Replace, true),
                (StepRole::Write, true),
                (StepRole::Claim, true),
                (StepRole::Write, true),
                (StepRole::Write, true),
                (StepRole::Write, true),
                (StepRole::Write, false),
            ]
        );
    }
}

/// The streams of the Entities intents state types for take the evolution's
/// key from what its insert returned, never from a read of the evolutions,
/// so an evolution another writer committed while the statement ran still
/// gets its streams, and every intent of such an Entity is read.
#[test]
fn the_streams_of_a_typed_entity_take_the_key_its_evolution_insert_returned() {
    let evolving = evolving();
    let grains = common_table(&evolving, "grains");
    assert!(
        grains.contains(concat!(
            r#"FROM (SELECT "evolved"."entity_key" AS "entity_key", "e"."attribute_name_key" AS "attribute_name_key","#,
            r#" "e"."dataset_id" AS "dataset_id", "evolved"."created" AS "created""#
        )),
        "{grains}"
    );
    assert!(
        grains.contains(r#"GROUP BY "ah"."entity_key", "ah"."attribute_name_key", "ah"."dataset_id", "ah"."created""#),
        "one row a grain: {grains}"
    );
    assert!(
        grains.contains(r#"WHERE "evolved"."id" = "i"."entity_id" AND "e"."entry_kind" IN (1, 2)"#),
        "{grains}"
    );
    assert!(!grains.contains("entities") && !grains.contains(r#"cardinality("i"."#), "{grains}");
}

/// A stream is inserted with the combinations and the box of the instances
/// its entries record, so the mask and extent growth after the history finds
/// it complete and writes the row no second time (clause 5.7.4.4).
#[test]
fn a_stream_is_inserted_with_the_shapes_and_extent_of_its_entries() {
    let evolving = evolving();
    let grains = common_table(&evolving, "grains");
    assert!(
        grains.contains(r#"COALESCE(bit_or((B'1'::bit(80) >> ((("ah"."attribute_kind" - 1) * (10))"#),
        "{grains}"
    );
    assert!(grains.contains(r#"ST_Envelope(ST_Collect("ah"."extent_geometry"))"#), "{grains}");
    assert!(
        evolving.contains(r#"SELECT "grains"."entity_key", "grains"."attribute_name_key", "grains"."dataset_id", "grains"."shape_mask", "grains"."value_extent" FROM "grains""#),
        "{evolving}"
    );
}

/// The streams are keyed in the order the history insert reaches their
/// entries, so the history rows arrive in ascending stream key and every
/// index led by it is extended at its right edge.
#[test]
fn the_streams_of_a_typed_entity_are_keyed_in_the_order_its_entries_are_recorded() {
    let evolving = evolving();
    let grains = common_table(&evolving, "grains");
    assert!(
        grains.contains(r#""i"."intent_key" AS "first_intent", "i"."base_ordinal" + ("e"."ordinality" - 1) AS "first_ordinal""#),
        "{grains}"
    );
    assert!(grains.contains(r#"MIN("ah"."first_intent"), MIN("ah"."first_ordinal")"#), "{grains}");
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
        streams.contains(r#"("entity_key", "attribute_name_key", "dataset_id", "shape_mask", "value_extent") SELECT "ah"."entity_key", "ah"."attribute_name_key", "ah"."dataset_id","#),
        "{streams}"
    );
    assert!(
        streams.contains(r#"FROM (SELECT "te"."entity_key" AS "entity_key", "e"."attribute_name_key" AS "attribute_name_key""#),
        "every stream carries its evolution's key: {streams}"
    );
    assert!(
        streams.contains(
            r#"ON CONFLICT ("entity_key", "attribute_name_key", COALESCE("dataset_id", '')) DO NOTHING RETURNING "entity_key", "attribute_name_key") "#
        ),
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
        observation(
            TemporalValue::Number {
                number: JsonNumber::from(1),
                datatype: None,
            },
            InstanceIdentity::generate(),
            None,
        ),
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
/// follows can bind each replaced row's stream, write clock and `instanceId`.
#[test]
fn a_replacement_reads_the_segment_write_clock_and_instance_id_of_each_replaced_row() {
    let instances = in_role(StepRole::Replace);
    assert!(
        instances.starts_with(r#"SELECT "attribute_history"."stream_key", "attribute_history"."recorded_at", "attribute_history"."verbatim_instance_id" FROM"#),
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
    assert!(claims.contains(r#"("entity_key", "attribute_name_key", "verbatim_instance_id")"#), "{claims}");
    assert!(!claims.contains("ON CONFLICT"), "{claims}");
}

/// The prefix of the extent growth, in the live branch.
const GROWTH: &str = r#"UPDATE "temporal_attribute_streams" SET "value_extent" = "#;

const SHAPE_GROWTH: &str = r#"UPDATE "temporal_attribute_streams" SET "shape_mask" = "#;

/// The shape mask growth of the drain.
fn shape_growth() -> String {
    rendered_steps()
        .into_iter()
        .map(|(_, sql)| sql)
        .find(|sql| sql.starts_with(SHAPE_GROWTH))
        .expect("a shape mask growth")
}

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
fn the_extent_grows_from_the_reach_else_the_geometry_the_history_row_stores() {
    let sql = growth();
    assert!(
        sql.starts_with(concat!(
            r#"UPDATE "temporal_attribute_streams" SET "value_extent" = "#,
            r#"ST_Envelope(ST_Collect("temporal_attribute_streams"."value_extent", "grown"."extent")) FROM (SELECT "s"."stream_key", "#,
            r#"ST_Envelope(ST_Collect(COALESCE("e"."value_geometry_reach", "e"."value_geometry"))) AS "extent" FROM "#
        )),
        "{sql}"
    );
    assert!(
        writing("attribute_history")
            .contains(r#", "e"."value_geometry", (CASE WHEN (ST_IsValid("e"."value_geometry")) THEN NULL ELSE ST_MakeValid("e"."value_geometry") END), "#)
    );
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
fn attribute_history_is_inserted_without_a_conflict_target_and_scope_history_under_its_write_identity() {
    let history = writing("attribute_history");
    assert!(!history.contains("ON CONFLICT"), "{history}");
    let scope = writing("scope_history");
    assert!(
        scope.contains(r#"ON CONFLICT ("recorded_at", "write_id", "write_ordinal") DO NOTHING"#),
        "{scope}"
    );
    assert!(
        history.contains(r#"(CASE WHEN (ST_IsValid("e"."value_geometry")) THEN NULL ELSE ST_MakeValid("e"."value_geometry") END)"#),
        "{history}"
    );
    assert!(scope.contains(r#""e"."scope_path"::ltree"#), "{scope}");
}
