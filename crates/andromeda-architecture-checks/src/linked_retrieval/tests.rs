use crate::linked_retrieval::source_violations;
use pretty_assertions::assert_eq;

/// A source outside every owner.
const OUTSIDE: &str = "crates/andromeda-notification/src/x.rs";

fn violations(path: &str, source: &str) -> Vec<String> {
    let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
    source_violations(path, &syntax)
}

#[test]
fn a_nested_term_read_outside_the_owner_is_flagged() {
    assert_eq!(violations(OUTSIDE, "fn f(t: &T) { t.linked_terms(); }").len(), 1);
}

#[test]
fn a_nested_term_read_inside_the_owner_passes() {
    assert_eq!(
        violations("crates/andromeda-linked-retrieval/src/narrowing/list.rs", "fn f(t: &T) { t.linked_terms(); }"),
        Vec::<String>::new()
    );
}

#[test]
fn a_depth_check_outside_the_owner_is_flagged() {
    assert_eq!(
        violations(
            "crates/andromeda-http-entity-map/src/validation.rs",
            "fn f(term: &T) -> u32 { term.linked_depth() }"
        )
        .len(),
        1
    );
}

#[test]
fn a_term_match_outside_the_owner_is_flagged() {
    assert_eq!(violations(OUTSIDE, "fn f() { let _ = matching::matching_term(a, b, c); }").len(), 1);
}

#[test]
fn a_join_mode_match_outside_the_owner_is_flagged() {
    let source = "fn f(m: Option<JoinMode>) -> u8 { match m { Some(JoinMode::Flat) => 1, _ => 0 } }";
    assert_eq!(violations(OUTSIDE, source).len(), 1);
}

#[test]
fn a_join_mode_inside_matches_is_flagged() {
    let source = "fn f(m: RetrievingJoin) -> bool { matches!(m, RetrievingJoin::Inline) }";
    assert_eq!(violations(OUTSIDE, source).len(), 1);
}

#[test]
fn a_join_mode_match_in_header_decoding_passes() {
    let source = "fn f(m: Option<JoinMode>) -> u8 { match m { Some(JoinMode::Flat) => 1, _ => 0 } }";
    assert_eq!(violations("crates/andromeda-http-headers/src/ngsild_join.rs", source), Vec::<String>::new());
}

#[test]
fn building_a_join_mode_is_not_a_match() {
    assert_eq!(violations(OUTSIDE, "fn f() -> JoinMode { JoinMode::Flat }"), Vec::<String>::new());
}

#[test]
fn writing_an_inline_entity_member_outside_the_writers_is_flagged() {
    let source = "fn f(fields: &mut Map, v: Value) { fields.insert(compact_properties::ENTITY.to_owned(), v); }";
    assert_eq!(violations("crates/andromeda-http-temporal/src/join.rs", source).len(), 1);
}

#[test]
fn serializing_an_inline_member_outside_the_writers_is_flagged() {
    let source = "fn f<M: SerializeMap>(map: &mut M, v: &V) { map.serialize_entry(properties::ENTITY_LIST, v); }";
    assert_eq!(violations(OUTSIDE, source).len(), 1);
}

#[test]
fn reading_an_inline_member_outside_the_writers_passes() {
    let source = "use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties; \
                  fn f(name: &str, fields: &Map) -> bool { name == compact_properties::ENTITY || fields.get(compact_properties::ENTITY_LIST).is_some() }";
    assert_eq!(violations(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn writing_an_inline_entity_member_in_a_writer_passes() {
    let source = "fn f(fields: &mut Map, v: Value) { fields.insert(compact_properties::ENTITY.to_owned(), v); }";
    assert_eq!(
        violations("crates/andromeda-ngsi-ld-render/src/temporal/inline_join.rs", source),
        Vec::<String>::new()
    );
}

#[test]
fn building_an_attribute_index_outside_the_owner_is_flagged() {
    let source = "fn f(n: AttributeName, iri: IriBuf) { let mut i = AttributeIriIndex::new(); i.insert(n, iri); }";
    assert_eq!(violations("crates/andromeda-temporal/src/linked/x.rs", source).len(), 2);
}

#[test]
fn building_an_attribute_index_in_its_own_module_passes() {
    let source = "fn f(n: AttributeName, iri: IriBuf) { let mut i = AttributeIriIndex::new(); i.insert(n, iri); }";
    assert_eq!(
        violations("crates/andromeda-ngsi-ld-query/src/projection/attribute_iri_index.rs", source),
        Vec::<String>::new()
    );
}

#[test]
fn an_insert_in_a_source_naming_no_index_passes() {
    assert_eq!(violations(OUTSIDE, "fn f(m: &mut HashMap<u8, u8>) { m.insert(1, 2); }"), Vec::<String>::new());
}

#[test]
fn an_admission_error_built_outside_its_conversion_is_flagged() {
    let source = "fn f() -> ParameterError { ParameterError::FilterTraversalWithoutJoin }";
    assert_eq!(violations("crates/andromeda-ngsi-ld-request/src/query/entity/validation.rs", source).len(), 1);
}

#[test]
fn an_admission_error_built_in_its_conversion_passes() {
    let source = "impl From<AdmissionError> for ParameterError { fn from(e: AdmissionError) -> ParameterError { ParameterError::LinkedRetrievalUnavailable { operation: e.0, member: e.1 } } }";
    assert_eq!(violations("crates/andromeda-ngsi-ld-request/src/error.rs", source), Vec::<String>::new());
}

#[test]
fn an_admission_error_matched_on_passes() {
    let source = "fn f(e: &ParameterError) -> u8 { match e { ParameterError::FilterTraversalWithoutJoin => 1, ParameterError::LinkedRetrievalUnavailable { .. } => 2, _ => 0 } }";
    assert_eq!(violations("crates/andromeda-http-response/src/problem.rs", source), Vec::<String>::new());
}

#[test]
fn an_adapter_outside_the_registered_files_is_flagged() {
    assert_eq!(violations(OUTSIDE, "impl LinkedSource for NotificationSource {}").len(), 1);
}

#[test]
fn an_adapter_in_the_engine_outside_the_registered_files_is_flagged() {
    let source = "impl LinkedRepresentation for Shortcut {}";
    assert_eq!(violations("crates/andromeda-linked-retrieval/src/plan.rs", source).len(), 1);
}

#[test]
fn a_registered_adapter_passes() {
    assert_eq!(
        violations(
            "crates/andromeda-entity/src/query/core_linked_source.rs",
            "impl LinkedSource for CoreLinkedSource<'_> {}"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_private_walk_outside_the_engine_is_flagged() {
    assert_eq!(
        violations(
            "crates/andromeda-ngsi-ld-render/src/temporal/inline_join.rs",
            "struct JoinPath<'a> { at: &'a str }"
        )
        .len(),
        1
    );
    assert_eq!(violations(OUTSIDE, "fn resolve_linked_level() {}").len(), 1);
}

#[test]
fn a_walk_inside_the_engine_passes() {
    assert_eq!(
        violations("crates/andromeda-linked-retrieval/src/traversal.rs", "fn traverse() {}"),
        Vec::<String>::new()
    );
}

#[test]
fn test_modules_are_not_scanned() {
    let source = "#[cfg(test)] mod tests { fn f(t: &T) { t.linked_terms(); } }";
    assert_eq!(violations(OUTSIDE, source), Vec::<String>::new());
}
