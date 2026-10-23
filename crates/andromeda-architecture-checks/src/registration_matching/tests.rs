use crate::registration_matching::{
    RegistrationMatchingSite,
    rule::{Rule, stale_adapters},
    site,
    source_findings,
};
use pretty_assertions::assert_eq;

/// A source outside the engine and every adapter.
const OUTSIDE: &str = "crates/andromeda-federation/src/x.rs";

/// The import that marks a source as handling registrations.
const HANDLES_REGISTRATIONS: &str = "use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;";

fn sites(path: &str, source: &str) -> Vec<RegistrationMatchingSite> {
    let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
    source_findings(&syntax)
        .into_iter()
        .filter(|finding| !finding.rule.permits(path))
        .map(|finding| site(path, finding))
        .collect()
}

fn reports(path: &str, source: &str) -> Vec<String> {
    sites(path, source).iter().map(RegistrationMatchingSite::report).collect()
}

#[test]
fn a_planted_identifier_decision_fails_the_gate_with_its_file_and_line() {
    let source = format!(
        "{HANDLES_REGISTRATIONS}\nfn covers(block: &RegistrationInfo, id: &str) -> bool {{\n    block.entities.iter().flatten().any(|info| info.id_pattern.as_ref().is_some_and(|pattern| pattern.is_match(id)))\n}}"
    );
    assert_eq!(
        reports(OUTSIDE, &source),
        vec![
            "crates/andromeda-federation/src/x.rs:3: reads a registered `id_pattern` to decide an identifier match".to_owned(),
            "crates/andromeda-federation/src/x.rs:3: calls `is_match` to decide an identifier match".to_owned(),
        ]
    );
}

#[test]
fn an_identifier_pattern_read_in_a_source_handling_no_registration_passes() {
    let source = "use andromeda_ngsi_ld_query::entity_selection::EntitySelector; fn f(s: &EntitySelector, id: &str) -> bool { s.id_pattern.as_ref().is_some_and(|p| p.is_match(id)) }";
    assert_eq!(reports(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn a_subscription_selector_pattern_read_is_flagged() {
    let source = "use andromeda_ngsi_ld_model::subscription::entity_selector::EntitySelector; fn f(s: &EntitySelector) -> bool { s.id_pattern.is_some() }";
    assert_eq!(sites("crates/andromeda-notification/src/x.rs", source).len(), 1);
}

#[test]
fn a_pattern_read_in_a_function_reading_a_block_s_entities_is_flagged() {
    let source = format!(
        "{HANDLES_REGISTRATIONS}\nfn f(r: &R) -> usize {{ r.information.iter().flat_map(|b| b.entities.iter().flatten()).filter(|s| s.id_pattern.is_some()).count() }}"
    );
    assert_eq!(sites(OUTSIDE, &source).len(), 1);
}

#[test]
fn a_query_pattern_read_beside_a_registration_passes() {
    let source = format!("{HANDLES_REGISTRATIONS}\nfn f(params: &Query) -> Option<&Pattern> {{ params.id_pattern.as_ref() }}");
    assert_eq!(reports(OUTSIDE, &source), Vec::<String>::new());
}

#[test]
fn writing_a_selection_pattern_passes() {
    let source = format!(
        "{HANDLES_REGISTRATIONS}\nfn f(block: &RegistrationInfo, selection: &mut Selection, pattern: &Pattern) {{ selection.id_pattern = Some(pattern.to_string()); }}"
    );
    assert_eq!(reports(OUTSIDE, &source), Vec::<String>::new());
}

#[test]
fn a_type_expression_matched_by_hand_is_flagged() {
    assert_eq!(
        sites(OUTSIDE, "fn f(info: &EntityInfo, types: &[T]) -> bool { info.type_.admits(types) }").len(),
        1
    );
    assert_eq!(
        sites(OUTSIDE, "fn f(s: &EntitySelector, types: &[T]) -> bool { s.type_.matches(types) }").len(),
        1
    );
}

#[test]
fn a_type_expression_matched_through_another_field_passes() {
    assert_eq!(
        reports(OUTSIDE, "fn f(s: &Selector, types: &[T]) -> bool { s.type_expr.matches(types) }"),
        Vec::<String>::new()
    );
}

#[test]
fn a_registration_scope_read_is_flagged() {
    assert_eq!(
        reports(OUTSIDE, "fn f(r: &R) -> bool { r.coverage.scope.is_some() }"),
        vec!["crates/andromeda-federation/src/x.rs:1: reads `coverage.scope` to decide a Scope match".to_owned()]
    );
}

#[test]
fn a_scope_read_off_another_owner_passes() {
    assert_eq!(reports(OUTSIDE, "fn f(e: &E) -> bool { e.scope.is_some() }"), Vec::<String>::new());
}

#[test]
fn a_second_csf_view_is_flagged() {
    assert_eq!(sites(OUTSIDE, "fn f(p: &P) -> bool { ContextSourceProperties::of(p).is_some() }").len(), 1);
}

#[test]
fn a_registration_expiry_compared_with_now_is_flagged() {
    let source =
        format!("{HANDLES_REGISTRATIONS}\nfn live(r: &R) -> bool {{ r.lifecycle.expires_at.is_none_or(|at| at > Utc::now()) && r.expires_at <= Utc::now() }}");
    assert_eq!(sites(OUTSIDE, &source).len(), 2);
}

#[test]
fn a_subscription_standing_expiry_compared_with_any_instant_is_flagged() {
    let source = "fn live(s: &CompiledSubscription, at: DateTime<Utc>) -> bool { s.standing.expires_at > Some(at) }";
    assert_eq!(sites("crates/andromeda-notification/src/x.rs", source).len(), 1);
}

#[test]
fn an_entity_expiry_method_or_a_cache_deadline_beside_a_registration_passes() {
    let source = format!(
        "{HANDLES_REGISTRATIONS}\nfn f(received: &R, entry: &E) -> bool {{ received.state.expires_at().is_some_and(|at| at.get() <= Utc::now()) || entry.expires_at <= Instant::now() }}"
    );
    assert_eq!(reports(OUTSIDE, &source), Vec::<String>::new());
}

#[test]
fn an_expiry_of_another_resource_compared_with_now_passes() {
    let source = "use andromeda_ngsi_ld_model::entity_map::EntityMap; fn live(m: &EntityMap) -> bool { m.expires_at > Utc::now() }";
    assert_eq!(reports(OUTSIDE, source), Vec::<String>::new());
}

#[test]
fn a_standing_decided_on_the_status_alone_is_flagged() {
    assert_eq!(sites(OUTSIDE, "fn f(s: &S) -> bool { s.status == SubscriptionStatus::Active }").len(), 1);
    assert_eq!(
        sites(OUTSIDE, "fn f(s: &S) -> bool { matches!(s.status, SubscriptionStatus::Active) }").len(),
        1
    );
}

#[test]
fn a_hand_built_identifier_axis_is_flagged() {
    let source = "fn f(s: &EntitySelector) -> bool { IdentifierAxis::of(s.id.as_slice(), s.id_pattern.as_ref()).admits(id) }";
    assert_eq!(sites("crates/andromeda-subscription/src/matching.rs", source).len(), 1);
}

#[test]
fn every_decision_inside_the_engine_passes() {
    let source = format!(
        "{HANDLES_REGISTRATIONS}\nfn f(s: &S, info: &EntityInfo, r: &R) -> bool {{ s.status == SubscriptionStatus::Active && s.expires_at.is_none_or(|at| at > Utc::now()) && info.type_.admits(t) && r.coverage.scope.is_some() && IdentifierAxis::of(&[], info.id_pattern.as_ref()).admits(id) }}"
    );
    for engine in [
        "crates/andromeda-ngsi-ld-model/src/subscription/standing.rs",
        "crates/andromeda-ngsi-ld-model/src/registration/selection.rs",
        "crates/andromeda-ngsi-ld-model/src/registration_matching/entity_specification.rs",
        "crates/andromeda-ngsi-ld-model/src/entity_info.rs",
    ] {
        assert_eq!(reports(engine, &source), Vec::<String>::new(), "{engine}");
    }
}

#[test]
fn the_identifier_definitions_may_build_an_axis_but_decide_no_standing() {
    let path = "crates/andromeda-ngsi-ld-query/src/entity_selection.rs";
    assert_eq!(
        reports(path, "fn f(s: &S) -> IdentifierAxis<'_> { IdentifierAxis::of(&s.ids, s.id_pattern.as_ref()) }"),
        Vec::<String>::new()
    );
    assert_eq!(sites(path, "fn f(s: &S) -> bool { s.status == SubscriptionStatus::Active }").len(), 1);
}

#[test]
fn test_modules_are_not_scanned() {
    let source =
        format!("{HANDLES_REGISTRATIONS}\n#[cfg(test)] mod tests {{ fn f(r: &R) -> bool {{ r.coverage.scope.is_some() && r.expires_at <= Utc::now() }} }}");
    assert_eq!(reports(OUTSIDE, &source), Vec::<String>::new());
}

#[test]
fn an_adapter_reading_nothing_its_rule_guards_is_stale() {
    let mapper = "crates/andromeda-postgresql-context-store/src/subscription_store/mapper.rs".to_owned();
    let stale = stale_adapters(&[(Rule::Standing, mapper.clone())]);
    assert!(stale.iter().all(|entry| !entry.contains(&mapper)), "{stale:?}");
    assert!(stale.iter().any(|entry| entry.contains("resolved_update.rs")), "{stale:?}");
    let misfiled = stale_adapters(&[(Rule::Expiry, mapper.clone())]);
    assert!(misfiled.iter().any(|entry| entry.contains(&mapper)), "{misfiled:?}");
}
