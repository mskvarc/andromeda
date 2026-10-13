use crate::{
    intent_application::{
        StepRole,
        application_steps,
        intent_source::{EntryNeed, IntentSource},
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
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent},
    scope_observation::ScopeObservation,
};
use pretty_assertions::assert_eq;
use sea_query::{Alias, DynIden, IntoIden};

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
    steps_over(&IntentSource::Queued(&[3, 5]), None)
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

/// Which needs a commit binding `intents` meets, in the order of
/// [`EntryNeed`].
fn needs_met(intents: &[TemporalIntentRecord]) -> Vec<bool> {
    let record = WriteRecord::restored(WriteId::mint(), recorded_at());
    let source = IntentSource::Bound { record: &record, intents };
    [
        EntryNeed::Any,
        EntryNeed::TypedIntent,
        EntryNeed::UntypedObservation,
        EntryNeed::AttributeEntry,
        EntryNeed::VerbatimInstance,
        EntryNeed::ScopeEntry,
    ]
    .map(|need| source.may_meet(need))
    .to_vec()
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

#[test]
fn the_commit_and_the_drain_render_one_statement_shape() {
    let drain: Vec<(StepRole, String)> = rendered_steps()
        .into_iter()
        .map(|(role, sql)| (role, sql.replace(QUEUED, "<source>")))
        .collect();
    let commit: Vec<(StepRole, String)> = commit_steps().into_iter().map(|(role, sql)| (role, sql.replace(BOUND, "<source>"))).collect();
    assert_eq!(drain, commit);
    assert!(drain.iter().all(|(_, sql)| sql.contains("<source>")), "{drain:?}");
}

#[test]
fn a_commit_binds_its_intents_as_one_typed_array() {
    for (_, sql) in commit_steps() {
        assert_eq!(sql.matches("$1::temporal_intent[]").count(), 1, "{sql}");
        assert!(sql.contains(BOUND), "{sql}");
        assert!(!sql.contains("temporal_append_intents"), "{sql}");
    }
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
    assert_eq!(needs_met(&[typed]), [true, true, false, true, false, false]);

    let claimed = record_of(&intent(vec![observation(
        TemporalValue::Number(JsonNumber::from(42)),
        InstanceIdentity::Verbatim("urn:x".parse().expect("an instance identifier")),
        None,
    )]));
    let scoped = record_of(&intent(vec![TemporalAppendEntry::Scope {
        observation: ScopeObservation::Path("/Madrid".parse::<ScopePath>().expect("a scope path")),
        observed_at: None,
        instance: InstanceIdentity::generate(),
    }]));
    assert_eq!(needs_met(&[claimed, scoped]), [true, true, false, true, true, true]);
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
    assert_eq!(needs_met(&[untyped(vec![tombstone()])])[2], false);
    assert_eq!(needs_met(&[untyped(Vec::new())])[2], true);
    let observing = untyped(vec![
        tombstone(),
        observation(TemporalValue::Number(JsonNumber::from(1)), InstanceIdentity::generate(), None),
    ]);
    assert_eq!(needs_met(&[observing])[2], true);
}

/// The drain never reads its entries back, so it runs every statement.
#[test]
fn a_queued_source_runs_every_statement() {
    let source = IntentSource::Queued(&[1]);
    assert!(
        [
            EntryNeed::TypedIntent,
            EntryNeed::UntypedObservation,
            EntryNeed::AttributeEntry,
            EntryNeed::VerbatimInstance,
            EntryNeed::ScopeEntry
        ]
        .into_iter()
        .all(|need| source.may_meet(need))
    );
}

#[test]
fn missing_evolutions_and_streams_are_created_before_the_history() {
    let evolutions = position(r#"INSERT INTO "temporal_entities""#);
    let streams = position(r#"INSERT INTO "temporal_attribute_streams""#);
    let history = position(r#"INSERT INTO "attribute_history""#);
    let scopes = position(r#"INSERT INTO "scope_history""#);
    assert!(evolutions < streams && streams < history && evolutions < scopes, "{:?}", rendered_steps());
    assert!(position(r#"INSERT INTO "write_records""#) < history);
    assert!(position(r#"DELETE FROM "attribute_history""#) < history, "the replaced instance goes first");
    assert!(position(r#"INSERT INTO "attribute_history_instance_claims""#) < history);
}

/// A source may hold several intents of one Entity, and a conflict action
/// changes a row once per statement, so the evolution takes the types of the
/// Entity's latest typed intent in one row.
#[test]
fn the_latest_typed_intent_of_an_entity_gives_its_evolution_its_types() {
    let evolutions = writing("temporal_entities");
    assert!(evolutions.contains(r#"SELECT DISTINCT ON ("i"."entity_id")"#), "{evolutions}");
    assert!(evolutions.contains(r#"ORDER BY "i"."entity_id" ASC, "i"."intent_key" DESC"#), "{evolutions}");
    assert!(evolutions.contains(r#"cardinality("i"."entity_type_name_keys") > 0"#), "{evolutions}");
    assert!(
        evolutions.contains(r#"DO UPDATE SET "modified_at" = "excluded"."modified_at", "entity_type_name_keys" = "excluded"."entity_type_name_keys""#),
        "{evolutions}"
    );
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
    assert!(
        position("SELECT") > position(r#"INSERT INTO "temporal_entities""#),
        "the source's own evolutions count"
    );
}

/// Clause 4.5.7: a client-supplied `instanceId` names one instance across
/// every `datasetId` of the attribute, so the row it replaces is sought on
/// every sibling stream, by that identifier and never by its instant.
#[test]
fn a_replaced_instance_is_sought_across_every_dataset_of_its_attribute() {
    let (_, replaced) = rendered_steps()
        .into_iter()
        .find(|(_, sql)| sql.starts_with(r#"DELETE FROM "attribute_history""#))
        .expect("the replacement");
    assert!(replaced.contains(r#""s2"."attribute_name_key" = "e"."attribute_name_key""#), "{replaced}");
    assert!(!replaced.contains(r#""s2"."dataset_id""#), "{replaced}");
    assert!(replaced.contains(r#""e"."verbatim_instance_id" IS NOT NULL"#), "{replaced}");
    assert!(!replaced.contains("observed_at") && !replaced.contains("generated_instance_id"), "{replaced}");
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
