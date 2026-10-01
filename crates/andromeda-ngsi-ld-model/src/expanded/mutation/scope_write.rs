//! How a fragment's `scope` lands on a stored entity, for every entity-wide
//! write that carries one: Update Attributes (clause 5.6.2.4), Append Attributes
//! (clause 5.6.3.4) and Merge Entity (clause 5.6.17.4).
//!
//! Clause 4.18 makes `scope` one non-reified member with one value: "There
//! shall at most be one instance of the scope property per Entity". Its
//! expanded array holds the Scopes of that one value, not attribute instances
//! told apart by `datasetId`, so it is never merged instance by instance. It is
//! either replaced as a whole, which is also what IETF RFC 7396 section 2 does
//! with any member that is not an object, or, when overwriting is not allowed,
//! extended with the Scopes it lacks.

use crate::expanded::mutation::deletion_sentinel::member_is_scope_deletion;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::{Map, Value};

/// Replace the target entity's `scope` member with the fragment's.
///
/// Clauses 5.6.2.4, 5.6.3.4 and 5.6.17.4 all say the scope of the target
/// "will become the one included in the Fragment" when overwriting is allowed.
/// The NGSI-LD Null that clause 4.18 reserves for a deleted scope removes the
/// member instead (clause 5.5.12), and removes nothing from a target without
/// one.
pub(crate) fn replace_scope(entity_obj: &mut Map<String, Value>, fragment_scope: &Value) {
    if member_is_scope_deletion(fragment_scope) {
        entity_obj.remove(properties::SCOPE.as_str());
    } else {
        // clone: the fragment is borrowed and the entity takes ownership of its scope.
        entity_obj.insert(properties::SCOPE.as_str().to_owned(), fragment_scope.clone());
    }
}

/// Append the fragment's Scopes the target does not already carry, in fragment
/// order.
///
/// Clause 5.6.3.4 under `noOverwrite`: "the Scopes in the Fragment that are not
/// part of the value of scope of the target Entity will be appended to the
/// value of the scope of the target Entity". A target without a scope takes
/// the fragment's whole.
pub(crate) fn append_missing_scopes(entity_obj: &mut Map<String, Value>, fragment_scope: &Value) {
    if let (Some(Value::Array(existing_scopes)), Some(fragment_scopes)) = (entity_obj.get_mut(properties::SCOPE.as_str()), fragment_scope.as_array()) {
        for scope in fragment_scopes {
            if !existing_scopes.contains(scope) {
                // clone: the fragment is borrowed and the scope array takes ownership.
                existing_scopes.push(scope.clone());
            }
        }
    } else {
        // clone: the fragment is borrowed and the entity takes ownership of its scope.
        entity_obj.insert(properties::SCOPE.as_str().to_owned(), fragment_scope.clone());
    }
}

#[cfg(test)]
mod tests {
    use crate::expanded::mutation::scope_write::{append_missing_scopes, replace_scope};
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use serde_json::{Map, Value, json};

    /// The expanded `scope` member holding the given paths, one value node each.
    fn scope(paths: &[&str]) -> Value {
        Value::Array(paths.iter().map(|path| json!({"@value": path})).collect())
    }

    /// An expanded entity object, with the given scope paths when there are any.
    fn entity(paths: Option<&[&str]>) -> Map<String, Value> {
        let mut entity = Map::new();
        entity.insert("@id".to_owned(), json!("urn:ngsi-ld:Camera:1"));
        if let Some(paths) = paths {
            entity.insert(properties::SCOPE.as_str().to_owned(), scope(paths));
        }
        entity
    }

    /// The scope member of an expanded entity object.
    fn stored_scope(entity: &Map<String, Value>) -> Option<&Value> {
        entity.get(properties::SCOPE.as_str())
    }

    /// Clause 5.6.17.4: the scope of the target "will become the one included
    /// in the Fragment", so no stored path outlives the replacement.
    #[test]
    fn replacing_a_scope_installs_the_fragment_value_whole() {
        let mut target = entity(Some(&["/X/A", "/Y/A", "/Z/A"]));

        replace_scope(&mut target, &scope(&["/A/B", "/C/D"]));

        pretty_assertions::assert_eq!(stored_scope(&target), Some(&scope(&["/A/B", "/C/D"])));
    }

    #[test]
    fn replacing_a_scope_array_with_one_path_leaves_only_that_path() {
        let mut target = entity(Some(&["/X/A", "/Y/A", "/Z/A"]));

        replace_scope(&mut target, &scope(&["/G/H"]));

        pretty_assertions::assert_eq!(stored_scope(&target), Some(&scope(&["/G/H"])));
    }

    /// A path the fragment shares with the target keeps the position the
    /// fragment gives it, and a stored path the fragment leaves out is gone.
    #[test]
    fn replacing_a_scope_keeps_a_path_the_fragment_repeats_in_its_fragment_position() {
        let mut target = entity(Some(&["/X/A", "/Y/A"]));

        replace_scope(&mut target, &scope(&["/X/A", "/Q/R"]));

        pretty_assertions::assert_eq!(stored_scope(&target), Some(&scope(&["/X/A", "/Q/R"])));
    }

    /// Clause 5.5.12: members of the Fragment that do not appear within the
    /// target "are added to the target".
    #[test]
    fn replacing_an_absent_scope_adds_it() {
        let mut target = entity(None);

        replace_scope(&mut target, &scope(&["/A/B", "/C/D"]));

        pretty_assertions::assert_eq!(stored_scope(&target), Some(&scope(&["/A/B", "/C/D"])));
    }

    /// Clause 4.18 reserves the NGSI-LD Null for a deleted scope, and clause
    /// 5.5.12 removes the target member a fragment sets to it.
    #[test]
    fn replacing_a_scope_with_the_ngsi_ld_null_removes_the_member() {
        let mut target = entity(Some(&["/X/A", "/Y/A"]));

        replace_scope(&mut target, &scope(&["urn:ngsi-ld:null"]));

        pretty_assertions::assert_eq!(stored_scope(&target), None);
    }

    /// Clause 5.6.3.4 under `noOverwrite`: only the Scopes the target lacks
    /// are appended, after the stored ones and in fragment order.
    #[test]
    fn appending_missing_scopes_adds_only_paths_the_target_lacks_in_fragment_order() {
        let mut target = entity(Some(&["/A/B", "/C/D"]));

        append_missing_scopes(&mut target, &scope(&["/A/B", "/E/F", "/G/H"]));

        pretty_assertions::assert_eq!(stored_scope(&target), Some(&scope(&["/A/B", "/C/D", "/E/F", "/G/H"])));
    }

    #[test]
    fn appending_missing_scopes_to_an_entity_without_scope_installs_the_fragment() {
        let mut target = entity(None);

        append_missing_scopes(&mut target, &scope(&["/A/B"]));

        pretty_assertions::assert_eq!(stored_scope(&target), Some(&scope(&["/A/B"])));
    }
}
