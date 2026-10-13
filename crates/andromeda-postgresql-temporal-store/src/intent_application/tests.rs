use crate::intent_application::{StepRole, application_steps};
use pretty_assertions::assert_eq;

/// The statements of an application over intents 3 and 5, in order.
fn rendered_steps() -> Vec<(StepRole, String)> {
    application_steps(&[3, 5])
        .expect("every statement builds")
        .into_iter()
        .map(|step| (step.role, step.sql))
        .collect()
}

/// The statement that writes into `table`.
fn writing(table: &str) -> String {
    let prefix = format!(r#"INSERT INTO "{table}""#);
    rendered_steps()
        .into_iter()
        .map(|(_, sql)| sql)
        .find(|sql| sql.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no statement writes {table}"))
}

/// The position of the first statement whose text starts with `prefix`.
fn position(prefix: &str) -> usize {
    rendered_steps()
        .iter()
        .position(|(_, sql)| sql.starts_with(prefix))
        .unwrap_or_else(|| panic!("no statement starts with {prefix}"))
}

#[test]
fn the_application_reads_the_entries_from_the_queue_rows_it_holds() {
    let history = writing("attribute_history");
    assert!(
        history.contains(r#"FROM "temporal_append_intents" AS "i", UNNEST("i"."entries") WITH ORDINALITY AS "e""#),
        "{history}"
    );
    assert!(history.contains(r#""i"."intent_key" = ANY($"#), "{history}");
    for (_, sql) in rendered_steps() {
        assert!(
            sql.contains(r#""temporal_append_intents" AS "i""#),
            "every statement reads the claimed rows: {sql}"
        );
        assert!(!sql.contains("payload") && !sql.contains("::json"), "{sql}");
    }
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

/// A batch may hold several intents of one Entity, and a conflict action
/// changes a row once per statement, so the evolution takes the types of the
/// Entity's latest typed intent in one row.
#[test]
fn the_latest_typed_intent_of_an_entity_gives_its_evolution_its_types() {
    let evolutions = writing("temporal_entities");
    assert!(evolutions.contains(r#"SELECT DISTINCT ON ("i"."entity_id")"#), "{evolutions}");
    assert!(evolutions.contains(r#"ORDER BY "i"."entity_id" ASC, "i"."intent_key" DESC"#), "{evolutions}");
    assert!(evolutions.contains(r#"cardinality("i"."entity_type_name_keys") > $"#), "{evolutions}");
    assert!(
        evolutions.contains(r#"DO UPDATE SET "modified_at" = "excluded"."modified_at", "entity_type_name_keys" = "excluded"."entity_type_name_keys""#),
        "{evolutions}"
    );
}

#[test]
fn entries_are_numbered_from_the_base_ordinal() {
    for table in ["attribute_history", "scope_history"] {
        let sql = writing(table);
        assert!(sql.contains(r#""i"."base_ordinal" + ("e"."ordinality" - $"#), "{sql}");
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
    assert!(sql.contains(r#"(NOT cardinality("i"."entity_type_name_keys") > $"#), "{sql}");
    assert!(
        sql.contains(r#""e"."entry_kind" <> $"#),
        "an intent holding an entry that ends nothing observes: {sql}"
    );
    assert!(
        sql.contains("(NOT EXISTS(SELECT $") && sql.contains(r#"FROM "temporal_entities" AS "te" WHERE "te"."id" = "i"."entity_id"))"#),
        "{sql}"
    );
    assert!(
        position("SELECT") > position(r#"INSERT INTO "temporal_entities""#),
        "the batch's own evolutions count"
    );
}

/// Clause 4.5.7: a client-supplied `instanceId` names one instance across
/// every `datasetId` of the attribute, so the row it replaces is sought on
/// every sibling stream.
#[test]
fn a_replaced_instance_is_sought_across_every_dataset_of_its_attribute() {
    let (_, replaced) = rendered_steps()
        .into_iter()
        .find(|(_, sql)| sql.starts_with(r#"DELETE FROM "attribute_history""#))
        .expect("the replacement");
    assert!(replaced.contains(r#""s2"."attribute_name_key" = "e"."attribute_name_key""#), "{replaced}");
    assert!(!replaced.contains(r#""s2"."dataset_id""#), "{replaced}");
    assert!(replaced.contains(r#""e"."verbatim_instance_id" IS NOT NULL"#), "{replaced}");
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
