//! The `scope` member of an expanded entity payload (ETSI GS CIM 009 clause 4.18).
//!
//! Clause 4.18 gives the member two readings under one grammar: the scope paths
//! an entity belongs to, and the NGSI-LD Null, of which the clause says the
//! special string "shall be only used and only appear in case of deleted
//! scopes". The two are different requests — one places the entity somewhere,
//! the other ends its placement — so this module tells them apart once and
//! hands every caller a [`ScopeMember`] rather than an opaque array.

use crate::expanded::{node::expanded_nodes, null_sentinel::member_is_null_sentinel};
use andromeda_ngsi_ld_lexicon::scope::path::{ScopePath, ScopePathError};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;
use thiserror::Error;

/// What clause 4.18 admits as the value of an entity's `scope` member.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScopeMember {
    /// The scope paths the entity belongs to, in the order the member lists
    /// them. Clause 4.18 lets an entity be "assigned to one or more Scopes at
    /// the same time", and represents more than one as a JSON array.
    Paths(Vec<ScopePath>),
    /// The NGSI-LD Null: the entity's scope is deleted (clause 4.18).
    Deleted,
}

/// Why the `scope` member of an expanded payload is not a value clause 4.18 admits.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ScopeMemberError {
    /// The member is not the array an expanded payload wraps every value in.
    #[error("the scope member is not an array of expanded values")]
    NotArrayValued,
    /// One node of the member carries no string value.
    #[error("a scope value is not a string")]
    NotAString,
    /// One value is not a scope path.
    #[error("a scope value is not a scope path: {source}")]
    NotAScopePath {
        /// Why it is not a scope path.
        source: ScopePathError,
    },
    /// The NGSI-LD Null appears beside a scope path.
    ///
    /// Clause 4.18 admits the special string only "in case of deleted scopes",
    /// which is a statement about the whole member: a request cannot delete the
    /// scope and set it in one breath.
    #[error("the NGSI-LD Null names a deleted scope, so it cannot appear beside a scope path")]
    NullBesidePath,
    /// The NGSI-LD Null appears more than once.
    ///
    /// Clause 4.18: "There shall at most be one instance of the scope property
    /// per Entity", so there is at most one deletion to state.
    #[error("the NGSI-LD Null appears more than once in the scope member, which states at most one deleted scope")]
    RepeatedNull,
}

/// Read the `scope` member of an expanded entity payload or fragment.
///
/// Returns `Ok(None)` when the payload carries no `scope` member at all, which
/// clause 4.18 makes the ordinary case: "The use of Scopes is optional".
///
/// # Errors
///
/// Returns [`ScopeMemberError`] when the member is shaped in a way clause 4.18
/// does not admit, which the write paths answer with the `BadRequestData` of
/// clause 5.5.4.
pub fn read_scope_member(expanded: &Value) -> Result<Option<ScopeMember>, ScopeMemberError> {
    let mut member = None;
    for node in expanded_nodes(expanded) {
        let Some(nodes) = scope_member_nodes(node)? else {
            continue;
        };
        let read = read_scope_nodes(nodes)?;
        member = member.or(Some(read));
    }
    Ok(member)
}

/// Read one node's `scope` member, which clause 4.18 makes either the paths the
/// entity belongs to or the NGSI-LD Null of a deleted scope.
fn read_scope_nodes(nodes: &[Value]) -> Result<ScopeMember, ScopeMemberError> {
    let mut paths = Vec::with_capacity(nodes.len());
    let mut deletions = 0_usize;
    for node in nodes {
        if member_is_null_sentinel(node) {
            deletions += 1;
            continue;
        }
        let raw = scope_value(node).ok_or(ScopeMemberError::NotAString)?;
        let path = raw.parse::<ScopePath>().map_err(|source| ScopeMemberError::NotAScopePath { source })?;
        paths.push(path);
    }

    if deletions == 0 {
        return Ok(ScopeMember::Paths(paths));
    }
    if !paths.is_empty() {
        return Err(ScopeMemberError::NullBesidePath);
    }
    if deletions > 1 {
        return Err(ScopeMemberError::RepeatedNull);
    }
    Ok(ScopeMember::Deleted)
}

/// Every scope path the expanded payload's `scope` member carries, in either of
/// the two shapes clause 4.18 gives it.
///
/// Clause 4.18: the Scope "is non-reified in the normalized Entity
/// representation and reified in the temporal representation of an Entity". The
/// two put the path at different depths — a bare value node in the first, the
/// `hasValue` of a Property instance in the second, and the first element of a
/// clause 4.5.9 pair in the `temporalValues` projection of that — so the member
/// is walked to whatever depth it puts them at. Clause 5.7.4.4 is what makes
/// the reified reading load-bearing: under `splitEntities` the Scope query
/// leaves the local temporal query and is applied to the merged Entities, which
/// carry the reified form.
///
/// Lenient by design, because this reads state the broker itself stored and
/// state a query matches against: a value that is not a scope path — the
/// NGSI-LD Null of a deleted scope, and the Temporal Property instants the
/// reified form carries beside the value, included — places the entity nowhere,
/// so it is skipped rather than reported. The clause 4.18 grammar is what tells
/// them apart, and it admits neither the sentinel nor a `DateTime`. A write
/// validates the same member through [`read_scope_member`] before it is ever
/// stored, and that reading stays confined to the non-reified shape a request
/// may carry.
#[must_use]
pub fn extract_scopes(expanded: &Value) -> Vec<ScopePath> {
    let mut paths = Vec::new();
    for node in expanded_nodes(expanded) {
        let Some(members) = scope_member_nodes(node).ok().flatten() else {
            continue;
        };
        for member in members {
            collect_scope_paths(member, &mut paths);
        }
    }
    paths
}

/// Collect every scope path one node of the `scope` member carries, at whatever
/// depth the representation puts it.
fn collect_scope_paths(node: &Value, paths: &mut Vec<ScopePath>) {
    match node {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        Value::Array(members) => {
            for member in members {
                collect_scope_paths(member, paths);
            }
        }
        Value::Object(members) => {
            if let Some(raw) = scope_value(node) {
                if let Ok(path) = raw.parse::<ScopePath>() {
                    paths.push(path);
                }
                return;
            }
            for value in members.values() {
                collect_scope_paths(value, paths);
            }
        }
    }
}

/// The nodes of one expanded node's `scope` member, when it carries one.
fn scope_member_nodes(node: &Value) -> Result<Option<&Vec<Value>>, ScopeMemberError> {
    let Some(object) = node.as_object() else {
        return Ok(None);
    };
    let Some(member) = object.get(properties::SCOPE.as_str()) else {
        return Ok(None);
    };
    member.as_array().map(Some).ok_or(ScopeMemberError::NotArrayValued)
}

/// The string one node of the expanded `scope` member carries.
fn scope_value(node: &Value) -> Option<&str> {
    node.as_object().and_then(|members| members.get("@value")).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use crate::expanded::scope_member::{ScopeMember, ScopeMemberError, extract_scopes, read_scope_member};
    use andromeda_ngsi_ld_lexicon::scope::path::{ScopePath, ScopePathError};
    use serde_json::{Value, json};

    /// One expanded entity whose `scope` member holds `member`.
    fn entity_scoped(member: &Value) -> Value {
        json!([{"@id": "urn:ngsi-ld:Vehicle:1", "https://uri.etsi.org/ngsi-ld/scope": member}])
    }

    /// The expanded form of a scope written as `raw`.
    fn node(raw: &str) -> Value {
        json!({"@value": raw})
    }

    #[test]
    fn a_payload_without_a_scope_member_carries_no_scope() {
        let entity = json!([{"@id": "urn:ngsi-ld:Vehicle:1"}]);
        pretty_assertions::assert_eq!(read_scope_member(&entity).expect("a payload may omit scope"), None);
        pretty_assertions::assert_eq!(extract_scopes(&entity), Vec::<ScopePath>::new());
    }

    /// Clause 4.18: "an Entity can be assigned to one or more Scopes at the
    /// same time. [...] The value of scope is represented as a JSON array in
    /// case there is more than one Scope."
    #[test]
    fn the_scope_member_reads_as_the_paths_it_lists() {
        let entity = entity_scoped(&json!([node("/Madrid"), node("/Traffic")]));
        let ScopeMember::Paths(paths) = read_scope_member(&entity).expect("valid scopes").expect("a scope member") else {
            panic!("a member of paths is not a deletion");
        };
        pretty_assertions::assert_eq!(paths.iter().map(ScopePath::as_str).collect::<Vec<&str>>(), vec!["/Madrid", "/Traffic"]);
        pretty_assertions::assert_eq!(extract_scopes(&entity), paths);
    }

    /// Clause 4.18: the special string "urn:ngsi-ld:null" "shall be only used
    /// and only appear in case of deleted scopes".
    #[test]
    fn the_ngsi_ld_null_reads_as_a_deleted_scope() {
        let entity = entity_scoped(&json!([node("urn:ngsi-ld:null")]));
        pretty_assertions::assert_eq!(
            read_scope_member(&entity).expect("the null is a value clause 4.18 admits"),
            Some(ScopeMember::Deleted)
        );
    }

    /// A deleted scope is nowhere, so the projection a store indexes and a
    /// `scopeQ` matches carries no path for it — never a path named after the
    /// sentinel, which no label-path index could hold anyway.
    #[test]
    fn a_deleted_scope_contributes_no_path_to_the_projection() {
        pretty_assertions::assert_eq!(extract_scopes(&entity_scoped(&json!([node("urn:ngsi-ld:null")]))), Vec::<ScopePath>::new());
    }

    #[test]
    fn the_null_beside_a_path_is_refused() {
        let entity = entity_scoped(&json!([node("/Madrid"), node("urn:ngsi-ld:null")]));
        pretty_assertions::assert_eq!(
            read_scope_member(&entity).expect_err("clause 4.18 admits the null only for a deleted scope"),
            ScopeMemberError::NullBesidePath
        );
    }

    /// Clause 4.18: "There shall at most be one instance of the scope property
    /// per Entity", so one member cannot state two deletions.
    #[test]
    fn a_repeated_null_is_refused() {
        let entity = entity_scoped(&json!([node("urn:ngsi-ld:null"), node("urn:ngsi-ld:null")]));
        pretty_assertions::assert_eq!(
            read_scope_member(&entity).expect_err("one member states at most one deletion"),
            ScopeMemberError::RepeatedNull
        );
    }

    #[test]
    fn a_value_outside_the_clause_4_18_grammar_is_refused() {
        let entity = entity_scoped(&json!([node("/A/B-C")]));
        pretty_assertions::assert_eq!(
            read_scope_member(&entity).expect_err("a hyphen is not a scope level character"),
            ScopeMemberError::NotAScopePath {
                source: ScopePathError::Malformed,
            }
        );
    }

    /// A level beyond what a label-path index can hold is bad input, reported
    /// here rather than as the failure of a database cast.
    #[test]
    fn a_level_beyond_the_storable_bound_is_refused() {
        let raw = format!("/{}", "a".repeat(1001));
        let entity = entity_scoped(&json!([node(&raw)]));
        pretty_assertions::assert_eq!(
            read_scope_member(&entity).expect_err("a level beyond the bound cannot be stored"),
            ScopeMemberError::NotAScopePath {
                source: ScopePathError::LevelTooLong(1001),
            }
        );
    }

    #[test]
    fn a_scope_value_that_is_not_a_string_is_refused() {
        let entity = entity_scoped(&json!([{"@value": 42}]));
        pretty_assertions::assert_eq!(read_scope_member(&entity).expect_err("a scope is a string"), ScopeMemberError::NotAString);
    }

    /// The scope reader and the NGSI-LD Null screening walk the same nodes, so
    /// a payload the expander left with more than one node cannot carry a scope
    /// past one of them.
    #[test]
    fn every_node_of_an_expanded_payload_has_its_scope_member_read() {
        let payload = json!([
            {"@id": "urn:ngsi-ld:Vehicle:1"},
            {"@id": "urn:ngsi-ld:Vehicle:2", "https://uri.etsi.org/ngsi-ld/scope": [node("/A/B-C")]}
        ]);
        pretty_assertions::assert_eq!(
            read_scope_member(&payload).expect_err("the second node carries a value outside clause 4.18"),
            ScopeMemberError::NotAScopePath {
                source: ScopePathError::Malformed,
            }
        );

        let scoped = json!([
            {"@id": "urn:ngsi-ld:Vehicle:1"},
            {"@id": "urn:ngsi-ld:Vehicle:2", "https://uri.etsi.org/ngsi-ld/scope": [node("/Madrid")]}
        ]);
        pretty_assertions::assert_eq!(extract_scopes(&scoped), vec!["/Madrid".parse::<ScopePath>().expect("a valid scope path")]);
    }

    /// Clause 4.18 reifies the Scope in the temporal representation of an
    /// Entity, and clause 5.7.4.4 has the Scope query applied to the merged
    /// Entities under `splitEntities`, so the reified form has to answer it.
    #[test]
    fn the_reified_temporal_scope_reads_as_the_paths_its_value_carries() {
        let reified = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "/Madrid/Centro"}],
            "https://uri.etsi.org/ngsi-ld/observedAt": [
                {"@value": "2018-08-01T11:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}
            ]
        }]);
        pretty_assertions::assert_eq!(
            extract_scopes(&entity_scoped(&reified)),
            vec!["/Madrid/Centro".parse::<ScopePath>().expect("a valid scope path")]
        );
    }

    /// Clause 4.18: "The value of scope is represented as a JSON array in case
    /// there is more than one Scope", which annex C clause C.5.16 renders for
    /// Vehicle A8311.
    #[test]
    fn a_reified_scope_naming_several_paths_reads_as_all_of_them() {
        let reified = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "/Madrid/Centro"}, {"@value": "/Company123/UnitA"}]
        }]);
        pretty_assertions::assert_eq!(
            extract_scopes(&entity_scoped(&reified)),
            vec![
                "/Madrid/Centro".parse::<ScopePath>().expect("a valid scope path"),
                "/Company123/UnitA".parse::<ScopePath>().expect("a valid scope path"),
            ]
        );
    }

    /// Clause 4.5.9 collapses the reified Scope into `[value, timestamp]`
    /// pairs, so a `temporalValues` projection puts the path one list deeper
    /// still and the timestamp beside it — which the clause 4.18 grammar
    /// refuses, so only the path is read.
    #[test]
    fn the_temporal_values_projection_of_a_reified_scope_reads_as_its_paths_alone() {
        let collapsed = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValues": [{"@list": [
                {"@list": [{"@value": "/Madrid/Centro"}, {"@value": "2018-08-01T11:00:00Z"}]}
            ]}]
        }]);
        pretty_assertions::assert_eq!(
            extract_scopes(&entity_scoped(&collapsed)),
            vec!["/Madrid/Centro".parse::<ScopePath>().expect("a valid scope path")]
        );
    }

    /// A removed Scope renders as the NGSI-LD Null, which places the Entity
    /// nowhere however deeply the representation nests it.
    #[test]
    fn a_reified_removal_contributes_no_path() {
        let removed = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
            "https://uri.etsi.org/ngsi-ld/deletedAt": [
                {"@value": "2018-08-01T13:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}
            ]
        }]);
        pretty_assertions::assert_eq!(extract_scopes(&entity_scoped(&removed)), Vec::<ScopePath>::new());
    }

    /// The write-side reading stays confined to the non-reified shape a request
    /// carries: it is the validator of what a client may send, not a reader of
    /// what the broker renders back.
    #[test]
    fn the_write_side_reading_does_not_follow_the_reified_shape() {
        let reified = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "/Madrid/Centro"}]
        }]);
        pretty_assertions::assert_eq!(
            read_scope_member(&entity_scoped(&reified)).expect_err("a reified instance is not a scope a request may set"),
            ScopeMemberError::NotAString
        );
    }

    #[test]
    fn a_scope_member_that_is_not_array_valued_is_refused() {
        let entity = entity_scoped(&json!("/Madrid"));
        pretty_assertions::assert_eq!(
            read_scope_member(&entity).expect_err("expansion wraps every value in an array"),
            ScopeMemberError::NotArrayValued
        );
        pretty_assertions::assert_eq!(extract_scopes(&entity), Vec::<ScopePath>::new());
    }
}
