//! Operations on expanded JSON-LD entities following NGSI-LD semantics (ETSI GS CIM 009 clause 4.6.2).
//!
//! All functions in this module operate on the **expanded** form of JSON-LD
//! (full IRIs, array-wrapped values) as stored in the database.  They are pure
//! functions with no I/O — the caller is responsible for loading and persisting
//! the entity.

use crate::{domain::geometry::Wkb, jsonld::vocab};
use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use iri_rs::Iri;
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Null sentinel detection
// ---------------------------------------------------------------------------

/// Returns `true` if the expanded attribute instance is a deletion sentinel
/// (`urn:ngsi-ld:null` or its expanded IRI form in hasValue, hasObject, or
/// hasLanguageMap).
pub fn instance_is_null_sentinel(instance: &Value) -> bool {
    let Some(obj) = instance.as_object() else {
        return false;
    };

    let check_value_null = |key: &str, value_key: &str| -> bool {
        obj.get(key)
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .and_then(|o| o.get(value_key))
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == vocab::expanded::NULL_URI.as_str() || s == vocab::expanded::NULL_IRI.as_str())
    };

    check_value_null(vocab::expanded::VALUE.as_str(), "@value")
        || check_value_null(vocab::expanded::OBJECT.as_str(), "@id")
        || check_value_null(vocab::expanded::LANGUAGE_MAP.as_str(), "@value")
}

// ---------------------------------------------------------------------------
// Dataset ID extraction
// ---------------------------------------------------------------------------

/// Extract the `datasetId` (`@id` value) from an expanded attribute instance.
pub fn get_instance_dataset_id(instance: &Value) -> Option<String> {
    instance
        .get(vocab::expanded::DATASET_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("@id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

// ---------------------------------------------------------------------------
// Expanded object accessors
// ---------------------------------------------------------------------------

/// Get a mutable reference to the first object in an expanded JSON-LD value.
///
/// Expanded JSON-LD wraps the top-level node in an array: `[{ ... }]`.  This
/// helper handles both the array wrapper and bare object forms.
pub fn expanded_object_mut(entity: &mut Value) -> Option<&mut Map<String, Value>> {
    match entity {
        Value::Array(arr) => arr.first_mut().and_then(Value::as_object_mut),
        Value::Object(obj) => Some(obj),
        _ => None,
    }
}

/// Get a shared reference to the first object in an expanded JSON-LD value.
pub fn expanded_object_ref(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Type extraction
// ---------------------------------------------------------------------------

/// Extract `@type` IRIs from an expanded entity payload.
pub fn extract_types(expanded: &Value) -> Vec<String> {
    expanded_object_ref(expanded)
        .and_then(|obj| obj.get("@type"))
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Scope extraction
// ---------------------------------------------------------------------------

/// Extract scope strings from an expanded entity payload.
pub fn extract_scopes(expanded: &Value) -> Vec<String> {
    let Some(obj) = expanded_object_ref(expanded) else {
        return Vec::new();
    };
    let Some(scope_value) = obj.get(vocab::expanded::SCOPE.as_str()) else {
        return Vec::new();
    };
    match scope_value {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_object().and_then(|obj| obj.get("@value")).and_then(|v| v.as_str()).map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// expiresAt extraction
// ---------------------------------------------------------------------------

/// Extract the `expiresAt` value from an expanded JSON-LD entity payload
/// (ETSI GS CIM 009 clause 4.22).
pub fn extract_expires_at(expanded: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    let obj = expanded_object_ref(expanded)?;
    let dt_str = obj
        .get(vocab::expanded::EXPIRES_AT.as_str())?
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@value"))
        .and_then(|v| v.as_str())?;
    chrono::DateTime::parse_from_rfc3339(dt_str).ok().map(|dt| dt.with_timezone(&chrono::Utc))
}

// ---------------------------------------------------------------------------
// GeoProperty extraction (WKB-only public surface)
// ---------------------------------------------------------------------------

/// Extract the entity's `location` GeoProperty decoded into canonical [`Wkb`].
pub fn extract_location_wkb(expanded: &Value) -> Option<Wkb> {
    extract_geo_property_wkb(expanded, vocab::expanded::LOCATION)
}

/// Extract the entity's `observationSpace` GeoProperty decoded into canonical [`Wkb`].
pub fn extract_observation_space_wkb(expanded: &Value) -> Option<Wkb> {
    extract_geo_property_wkb(expanded, vocab::expanded::OBSERVATION_SPACE)
}

/// Extract the entity's `operationSpace` GeoProperty decoded into canonical [`Wkb`].
pub fn extract_operation_space_wkb(expanded: &Value) -> Option<Wkb> {
    extract_geo_property_wkb(expanded, vocab::expanded::OPERATION_SPACE)
}

fn extract_geo_property_wkb(expanded: &Value, iri: Iri<&'static str>) -> Option<Wkb> {
    let geo_node = geo_node_for_property(expanded, iri)?;
    if let Some(literal) = geo_node.get("@value") {
        return literal_geojson_to_wkb(literal);
    }
    expanded_geo_node_to_wkb(geo_node)
}

/// Look up the inner GeoJSON node for an expanded GeoProperty attribute.
///
/// Returns the "{@type, ...coordinates}" object that lives at
/// `expanded[iri][..].hasValue[0]`, ignoring whether the value was inlined
/// as a literal `@value` or expanded as a nested JSON-LD GeoJSON node —
/// callers branch on the presence of `@value` to pick the decoding path.
fn geo_node_for_property<'a>(expanded: &'a Value, iri: Iri<&'static str>) -> Option<&'a Value> {
    let obj = expanded_object_ref(expanded)?;
    let prop = obj.get(iri.as_str())?;
    prop.as_array()
        .and_then(|arr| arr.last())
        .and_then(|inst| inst.get(vocab::expanded::VALUE.as_str()))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
}

/// Decode a literal-GeoJSON `@value` (string or already-compact object) into [`Wkb`].
fn literal_geojson_to_wkb(literal: &Value) -> Option<Wkb> {
    match literal {
        Value::String(s) => {
            let value: Value = serde_json::from_str(s).ok()?;
            Wkb::from_geojson_value(&value).ok()
        }
        Value::Object(_) => Wkb::from_geojson_value(literal).ok(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => None,
    }
}

/// Walk an expanded JSON-LD GeoJSON node directly into canonical [`Wkb`].
///
/// Replaces the legacy "reconstruct compact-GeoJSON string → re-parse →
/// `serde_json::from_value` clone → `geojson::Geometry`" round-trip with a
/// single-pass walker that emits `geo::Geometry<f64>` directly. No
/// intermediate `Value` cloning, no string serialisation, no second JSON
/// parser pass. Returns `None` for unknown geometry types or malformed
/// input so the caller can fall through to the slow path if needed.
pub fn expanded_geo_node_to_wkb(node: &Value) -> Option<Wkb> {
    let geometry = expanded_geo_node_to_geometry(node)?;
    Wkb::from_geo(&geometry).ok()
}

fn expanded_geo_node_to_geometry(node: &Value) -> Option<Geometry<f64>> {
    let obj = node.as_object()?;
    let geom_type = obj.get("@type")?.as_array()?.first()?.as_str()?.strip_prefix(vocab::prefix::GEOJSON.as_str())?;

    let coords_outer = obj.get(vocab::expanded::COORDINATES.as_str())?.as_array()?.first()?;

    match geom_type {
        "Point" => Some(Geometry::Point(Point::from(walk_position(coords_outer)?))),
        "MultiPoint" => {
            let points: Vec<Point<f64>> = walk_position_list(coords_outer)?.into_iter().map(Point::from).collect();
            Some(Geometry::MultiPoint(MultiPoint::new(points)))
        }
        "LineString" => Some(Geometry::LineString(LineString::from(walk_position_list(coords_outer)?))),
        "MultiLineString" => {
            let lines = walk_linestring_list(coords_outer)?;
            Some(Geometry::MultiLineString(MultiLineString::new(lines)))
        }
        "Polygon" => Some(Geometry::Polygon(walk_polygon(coords_outer)?)),
        "MultiPolygon" => {
            let polys = walk_polygon_list(coords_outer)?;
            Some(Geometry::MultiPolygon(MultiPolygon::new(polys)))
        }
        _other => None,
    }
}

/// Iterate the children of either a bare `Array` or an `{"@list": [...]}`
/// wrapper — the two shapes JSON-LD expansion produces for ordered lists.
fn list_items(node: &Value) -> Option<&[Value]> {
    if let Some(arr) = node.as_array() {
        return Some(arr.as_slice());
    }
    node.as_object().and_then(|o| o.get("@list")).and_then(|v| v.as_array()).map(Vec::as_slice)
}

/// Read a numeric leaf from either a bare JSON number or `{"@value": n}`.
fn scalar_number(node: &Value) -> Option<f64> {
    if let Some(n) = node.as_f64() {
        return Some(n);
    }
    node.as_object().and_then(|o| o.get("@value")).and_then(Value::as_f64)
}

fn walk_position(node: &Value) -> Option<Coord<f64>> {
    let items = list_items(node)?;
    if items.len() < 2 {
        return None;
    }
    let x = scalar_number(&items[0])?;
    let y = scalar_number(&items[1])?;
    Some(Coord { x, y })
}

fn walk_position_list(node: &Value) -> Option<Vec<Coord<f64>>> {
    let items = list_items(node)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(walk_position(item)?);
    }
    Some(out)
}

fn walk_linestring_list(node: &Value) -> Option<Vec<LineString<f64>>> {
    let items = list_items(node)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(LineString::from(walk_position_list(item)?));
    }
    Some(out)
}

fn walk_polygon(node: &Value) -> Option<Polygon<f64>> {
    let rings = list_items(node)?;
    if rings.is_empty() {
        return None;
    }
    let exterior = LineString::from(walk_position_list(&rings[0])?);
    let mut interiors = Vec::with_capacity(rings.len().saturating_sub(1));
    for ring in &rings[1..] {
        interiors.push(LineString::from(walk_position_list(ring)?));
    }
    Some(Polygon::new(exterior, interiors))
}

fn walk_polygon_list(node: &Value) -> Option<Vec<Polygon<f64>>> {
    let items = list_items(node)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(walk_polygon(item)?);
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// JSON merge
// ---------------------------------------------------------------------------

/// Merge `patch` into `base` following NGSI-LD expanded form conventions.
///
/// - Arrays are merged element-by-element (recursively).
/// - An object patch on an array-wrapped base merges into the first element.
/// - Object keys starting with `@` are never overwritten.
/// - Object members are replaced (not recursively merged), matching NGSI-LD
///   partial update semantics where attribute instance members are top-level atomic units.
/// - Scalars: replaced entirely.
pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Array(base_arr), Value::Array(patch_arr)) => {
            for (i, patch_elem) in patch_arr.iter().enumerate() {
                if i < base_arr.len() {
                    merge_json(&mut base_arr[i], patch_elem);
                } else {
                    base_arr.push(patch_elem.clone());
                }
            }
        }
        (Value::Array(base_arr), Value::Object(_)) => {
            if let Some(first) = base_arr.first_mut() {
                merge_json(first, patch);
            }
        }
        (Value::Object(base_obj), Value::Object(patch_obj)) => {
            for (key, value) in patch_obj {
                if key.starts_with('@') {
                    continue;
                }
                base_obj.insert(key.clone(), value.clone());
            }
        }
        (base, patch) => {
            *base = patch.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// Attribute-level operations
// ---------------------------------------------------------------------------

/// Deep-merge attribute instances into an entity object, matching by datasetId.
pub fn deep_merge_attribute(entity_obj: &mut Map<String, Value>, key: &str, patch_instances: &[Value]) {
    let existing = entity_obj.get_mut(key);
    let Some(existing) = existing else {
        entity_obj.insert(key.to_string(), Value::Array(patch_instances.to_vec()));
        return;
    };
    let Some(existing_arr) = existing.as_array_mut() else {
        entity_obj.insert(key.to_string(), Value::Array(patch_instances.to_vec()));
        return;
    };

    for patch_inst in patch_instances {
        let patch_ds_id = get_instance_dataset_id(patch_inst);

        let matched = existing_arr.iter_mut().find(|inst| get_instance_dataset_id(inst) == patch_ds_id);

        if let Some(existing_inst) = matched {
            // If the attribute type changed, replace the entire instance
            let type_changed = {
                let existing_type = existing_inst.get("@type");
                let patch_type = patch_inst.get("@type");
                match (existing_type, patch_type) {
                    (Some(et), Some(pt)) => et != pt,
                    _ => false,
                }
            };

            if type_changed {
                *existing_inst = patch_inst.clone();
            } else if let (Some(base_obj), Some(patch_obj)) = (existing_inst.as_object_mut(), patch_inst.as_object()) {
                for (k, v) in patch_obj {
                    if k.starts_with('@') {
                        continue;
                    }
                    base_obj.insert(k.clone(), v.clone());
                }
            }
        } else {
            existing_arr.push(patch_inst.clone());
        }
    }
}

/// Remove instances matching a dataset ID (or the default instance if `None`)
/// from the instances array for an attribute key.  Removes the key entirely if
/// no instances remain.
/// Returns `true` if at least one instance was actually removed.
fn remove_instances_by_dataset(entity_obj: &mut Map<String, Value>, key: &str, dataset_id: Option<&str>) -> bool {
    let Some(Value::Array(instances)) = entity_obj.get_mut(key) else {
        return false;
    };

    let before = instances.len();
    match dataset_id {
        Some(ds_id) => {
            instances.retain(|inst| get_instance_dataset_id(inst).as_deref() != Some(ds_id));
        }
        None => {
            // Remove the default instance (the one without datasetId)
            instances.retain(|inst| get_instance_dataset_id(inst).is_some());
        }
    }
    let removed = instances.len() < before;

    if instances.is_empty() {
        entity_obj.remove(key);
    }

    removed
}

// ---------------------------------------------------------------------------
// Entity-level mutation operations
// ---------------------------------------------------------------------------

/// NGSI-LD Merge Patch (clause 5.6.1): deep-merge `patch` into `entity`, handling
/// null sentinels for attribute deletion.
pub fn merge_entity(entity: &mut Value, patch: &Value) -> Result<(), String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;
    let patch_obj = expanded_object_ref(patch).ok_or("Patch data is not a valid JSON-LD expanded value")?;

    for (key, value) in patch_obj {
        // Merge @type arrays (union, deduplicate) so entity type additions work
        if key == "@type" {
            if let (Some(entity_types), Some(patch_types)) = (entity_obj.get_mut("@type").and_then(Value::as_array_mut), value.as_array()) {
                for t in patch_types {
                    if !entity_types.contains(t) {
                        entity_types.push(t.clone());
                    }
                }
            }
            continue;
        }
        if key.starts_with('@') {
            continue;
        }

        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                entity_obj.insert(key.clone(), value.clone());
                continue;
            }
        };

        if !patch_instances.iter().any(instance_is_null_sentinel) {
            deep_merge_attribute(entity_obj, key, patch_instances);
            continue;
        }

        // Process null sentinel instances
        for patch_instance in patch_instances {
            if instance_is_null_sentinel(patch_instance) {
                let dataset_id = get_instance_dataset_id(patch_instance);
                if dataset_id.is_none() {
                    // No datasetId on null sentinel -> delete ALL instances
                    entity_obj.remove(key);
                    break;
                } else {
                    remove_instances_by_dataset(entity_obj, key, dataset_id.as_deref());
                }
            }
        }
    }

    Ok(())
}

/// Append attributes (clause 5.6.3): add attributes from `attrs` to `entity`.
/// With `no_overwrite`, existing instances (matched by datasetId) are kept;
/// new instances with a different datasetId are still appended.
/// Without `no_overwrite`, instances are merged by datasetId.
///
/// Returns `(not_appended)` -- a list of attribute IRIs where at least one
/// instance was NOT appended (because `no_overwrite` was set and the instance
/// already existed).
pub fn append_entity_attrs(entity: &mut Value, attrs: &Value, no_overwrite: bool) -> Result<Vec<String>, String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;
    let attrs_obj = attrs.as_object().ok_or("Attributes must be a JSON object")?;

    let mut not_appended = Vec::new();

    for (key, value) in attrs_obj {
        if key.starts_with('@') {
            continue;
        }

        // Scope: merge arrays with deduplication
        if key == vocab::expanded::SCOPE.as_str() {
            if let (Some(Value::Array(existing_scopes)), Some(new_scopes)) = (entity_obj.get_mut(key), value.as_array()) {
                if no_overwrite {
                    // noOverwrite: add scope values that don't already exist
                    for s in new_scopes {
                        if !existing_scopes.contains(s) {
                            existing_scopes.push(s.clone());
                        }
                    }
                } else {
                    // overwrite: replace scope
                    entity_obj.insert(key.clone(), value.clone());
                }
            } else {
                entity_obj.insert(key.clone(), value.clone());
            }
            continue;
        }

        if !entity_obj.contains_key(key) {
            // Attribute doesn't exist yet -- just add it
            entity_obj.insert(key.clone(), value.clone());
            continue;
        }

        // Attribute already exists -- merge by datasetId
        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                if !no_overwrite {
                    entity_obj.insert(key.clone(), value.clone());
                } else {
                    not_appended.push(key.clone());
                }
                continue;
            }
        };

        if no_overwrite {
            // Only add instances whose datasetId doesn't already exist
            let Some(Value::Array(existing_arr)) = entity_obj.get_mut(key) else {
                entity_obj.insert(key.clone(), value.clone());
                continue;
            };
            let mut any_skipped = false;
            for patch_inst in patch_instances {
                let patch_ds = get_instance_dataset_id(patch_inst);
                let already_exists = existing_arr.iter().any(|inst| get_instance_dataset_id(inst) == patch_ds);
                if !already_exists {
                    existing_arr.push(patch_inst.clone());
                } else {
                    any_skipped = true;
                }
            }
            if any_skipped {
                not_appended.push(key.clone());
            }
        } else {
            deep_merge_attribute(entity_obj, key, patch_instances);
        }
    }

    Ok(not_appended)
}

/// Update entity attributes (clause 5.6.2).
///
/// For regular attributes: if the attribute exists, update by datasetId
/// matching; if not, add it. For scope: if scope doesn't exist on the
/// entity, report as not updated.
///
/// Returns a list of attribute IRIs that were NOT updated (e.g. scope not
/// present). The caller uses this to decide between 204 and 207.
pub fn update_entity_attrs(entity: &mut Value, attrs: &Value) -> Result<Vec<String>, String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;
    let attrs_obj = attrs.as_object().ok_or("Attributes must be a JSON object")?;

    let mut not_updated = Vec::new();

    for (key, value) in attrs_obj {
        // Merge @type arrays (union, deduplicate) so entity type additions work
        if key == "@type" {
            if let (Some(entity_types), Some(patch_types)) = (entity_obj.get_mut("@type").and_then(Value::as_array_mut), value.as_array()) {
                for t in patch_types {
                    if !entity_types.contains(t) {
                        entity_types.push(t.clone());
                    }
                }
            }
            continue;
        }
        if key.starts_with('@') {
            continue;
        }

        // Scope: only update if already present, otherwise report as not updated
        if key == vocab::expanded::SCOPE.as_str() {
            if entity_obj.contains_key(key) {
                entity_obj.insert(key.clone(), value.clone());
            } else {
                not_updated.push(key.clone());
            }
            continue;
        }

        // If the attribute doesn't exist yet, add it
        if !entity_obj.contains_key(key) {
            entity_obj.insert(key.clone(), value.clone());
            continue;
        }

        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                entity_obj.insert(key.clone(), value.clone());
                continue;
            }
        };

        if patch_instances.iter().any(instance_is_null_sentinel) {
            for patch_instance in patch_instances {
                if instance_is_null_sentinel(patch_instance) {
                    let dataset_id = get_instance_dataset_id(patch_instance);
                    remove_instances_by_dataset(entity_obj, key, dataset_id.as_deref());
                } else {
                    deep_merge_attribute(entity_obj, key, std::slice::from_ref(patch_instance));
                }
            }
        } else {
            // Update by datasetId: replace matching instances, preserve others
            deep_merge_attribute(entity_obj, key, patch_instances);
        }
    }

    Ok(not_updated)
}

/// Partial update of a single attribute (clause 5.6.4): merge `fragment` into the
/// attribute identified by `attr_iri`, optionally targeting a specific
/// `dataset_id`.
pub fn partial_update_attr(entity: &mut Value, attr_iri: &str, dataset_id: Option<&str>, fragment: &Value) -> Result<(), String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;

    if !entity_obj.contains_key(attr_iri) {
        return Err(format!("Attribute {} not found", attr_iri));
    }

    // datasetId selection is controlled by query parameter (datasetId).
    // A datasetId embedded in the payload does not select the target instance.
    let effective_ds: Option<&str> = dataset_id;

    if instance_is_null_sentinel(fragment) {
        remove_instances_by_dataset(entity_obj, attr_iri, effective_ds);
        return Ok(());
    }

    // Extract the fragment's @type (if present) for type-mismatch validation
    let fragment_types: Vec<&str> = fragment
        .get("@type")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    if let Some(ds_id) = effective_ds {
        let Some(Value::Array(instances)) = entity_obj.get_mut(attr_iri) else {
            return Err(format!("Attribute {} has no instances", attr_iri));
        };

        let found = instances.iter_mut().find(|instance| {
            instance
                .get(vocab::expanded::DATASET_ID.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@id"))
                .and_then(|id| id.as_str())
                == Some(ds_id)
        });

        match found {
            Some(instance) => {
                validate_type_match(&fragment_types, instance, attr_iri)?;
                merge_json(instance, fragment);
            }
            None => {
                return Err(format!("Attribute {} with datasetId {} not found", attr_iri, ds_id));
            }
        }
    } else {
        // No datasetId: target the default instance (the one without datasetId)
        let Some(Value::Array(instances)) = entity_obj.get_mut(attr_iri) else {
            let Some(attr_value) = entity_obj.get_mut(attr_iri) else {
                return Err(format!("Attribute {} not found", attr_iri));
            };
            if !fragment_types.is_empty() {
                validate_type_match(&fragment_types, attr_value, attr_iri)?;
            }
            merge_json(attr_value, fragment);
            return Ok(());
        };

        let found = instances.iter_mut().find(|instance| get_instance_dataset_id(instance).is_none());

        match found {
            Some(instance) => {
                validate_type_match(&fragment_types, instance, attr_iri)?;
                merge_json(instance, fragment);
            }
            None => {
                return Err(format!("Attribute {} default instance not found", attr_iri));
            }
        }
    }

    Ok(())
}

/// Check that the fragment's @type (if specified) matches the instance's @type.
fn validate_type_match(fragment_types: &[&str], instance: &Value, attr_iri: &str) -> Result<(), String> {
    if fragment_types.is_empty() {
        return Ok(());
    }
    let instance_types: Vec<&str> = instance
        .get("@type")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if instance_types.is_empty() {
        return Ok(());
    }
    // Check if fragment type intersects with instance type
    if fragment_types.iter().any(|ft| instance_types.contains(ft)) {
        Ok(())
    } else {
        Err(format!("Attribute type mismatch for {}", attr_iri))
    }
}

/// Delete a single attribute (clause 5.6.5).
///
/// - `delete_all`: remove the key entirely.
/// - `dataset_id`: remove only the matching instance.
/// - Neither: remove the default (no-datasetId) instance.
pub fn delete_attr(entity: &mut Value, attr_iri: &str, dataset_id: Option<&str>, delete_all: bool) -> Result<(), String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;

    if !entity_obj.contains_key(attr_iri) {
        return Err(format!("Attribute {} not found", attr_iri));
    }

    if delete_all {
        entity_obj.remove(attr_iri);
    } else if !remove_instances_by_dataset(entity_obj, attr_iri, dataset_id) {
        return match dataset_id {
            Some(ds_id) => Err(format!("Attribute {} with datasetId {} not found", attr_iri, ds_id)),
            None => Err(format!("Attribute {} default instance not found", attr_iri)),
        };
    }

    Ok(())
}

/// Replace a single attribute (clause 5.6.19): fully replaces the attribute value.
///
/// When `dataset_id` is provided (from query parameter), it specifies which instance to replace.
/// Otherwise, the datasetId from `value` is used to match. If neither specifies a datasetId,
/// the default instance (without datasetId) is replaced.
pub fn replace_attr(entity: &mut Value, attr_iri: &str, dataset_id: Option<&str>, value: &Value) -> Result<(), String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;

    let existing = entity_obj.get(attr_iri).ok_or_else(|| format!("Attribute {} not found", attr_iri))?;

    // Query param datasetId takes precedence over value's datasetId
    let target_ds: Option<String> = match dataset_id {
        Some(ds) => Some(ds.to_string()),
        None => get_instance_dataset_id(value),
    };

    let existing_instances = existing.as_array();

    if let Some(instances) = existing_instances {
        let mut new_instances: Vec<Value> = Vec::with_capacity(instances.len());
        let mut replaced = false;

        for inst in instances {
            let inst_ds = get_instance_dataset_id(inst);
            if inst_ds == target_ds {
                new_instances.push(value.clone());
                replaced = true;
            } else {
                new_instances.push(inst.clone());
            }
        }

        if !replaced {
            return match target_ds {
                Some(ds) => Err(format!("Attribute {} with datasetId {} not found", attr_iri, ds)),
                None => Err(format!("Attribute {} default instance not found", attr_iri)),
            };
        }

        entity_obj.insert(attr_iri.to_string(), Value::Array(new_instances));
    } else {
        entity_obj.insert(attr_iri.to_string(), Value::Array(vec![value.clone()]));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use chrono::Datelike;
    use serde_json::json;

    #[test]
    fn null_sentinel_has_value() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    #[test]
    fn null_sentinel_has_object() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:null"}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    #[test]
    fn non_null_is_not_sentinel() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
        });
        assert!(!instance_is_null_sentinel(&instance));
    }

    #[test]
    fn extract_dataset_id() {
        let instance = json!({
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
        });
        pretty_assertions::assert_eq!(get_instance_dataset_id(&instance).as_deref(), Some("urn:ngsi-ld:Dataset:A"));
    }

    #[test]
    fn extract_scopes_from_entity() {
        let entity = json!([{
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [
                {"@value": "/Madrid"},
                {"@value": "/Traffic"}
            ]
        }]);
        let scopes = extract_scopes(&entity);
        pretty_assertions::assert_eq!(scopes, vec!["/Madrid", "/Traffic"]);
    }

    #[test]
    fn merge_entity_adds_new_attr() {
        let mut entity = json!([{"@type": ["https://example.com/T"], "https://example.com/a": [{"@value": 1}]}]);
        let patch = json!([{"https://example.com/b": [{"@value": 2}]}]);
        merge_entity(&mut entity, &patch).unwrap();
        assert!(entity[0].get("https://example.com/b").is_some());
    }

    #[test]
    fn append_no_overwrite_skips_existing() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        let attrs = json!({"https://example.com/a": [{"@value": 99}], "https://example.com/b": [{"@value": 2}]});
        append_entity_attrs(&mut entity, &attrs, true).unwrap();
        pretty_assertions::assert_eq!(entity[0]["https://example.com/a"][0]["@value"], 1);
        pretty_assertions::assert_eq!(entity[0]["https://example.com/b"][0]["@value"], 2);
    }

    #[test]
    fn delete_attr_removes_key() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        delete_attr(&mut entity, "https://example.com/a", None, true).unwrap();
        assert!(entity[0].get("https://example.com/a").is_none());
    }

    #[test]
    fn delete_attr_not_found() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        let result = delete_attr(&mut entity, "https://example.com/missing", None, true);
        assert!(result.is_err());
    }

    #[test]
    fn extract_expires_at_from_expanded_entity() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "@type": ["https://example.com/Building"],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": "2030-12-31T23:59:59.000Z"
            }]
        }]);
        let result = extract_expires_at(&entity);
        assert!(result.is_some());
        pretty_assertions::assert_eq!(result.unwrap().year(), 2030);
    }

    #[test]
    fn extract_expires_at_missing() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "@type": ["https://example.com/Building"]
        }]);
        assert!(extract_expires_at(&entity).is_none());
    }

    #[test]
    fn extract_expires_at_invalid_date() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": "not-a-date"
            }]
        }]);
        assert!(extract_expires_at(&entity).is_none());
    }

    /// Build the expected canonical [`Wkb`] for a known compact-GeoJSON object
    /// by routing through the legacy `from_geojson_value` path; the direct
    /// walker must produce byte-equal output.
    fn wkb_from_compact(compact: &serde_json::Value) -> Wkb {
        Wkb::from_geojson_value(compact).expect("compact geojson decodes")
    }

    #[test]
    fn expanded_geo_node_walker_decodes_point_byte_equal_to_legacy() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}]}
            ]
        });
        let compact = json!({"type": "Point", "coordinates": [16.37_f64, 48.20_f64]});
        pretty_assertions::assert_eq!(expanded_geo_node_to_wkb(&expanded).expect("walker decodes Point"), wkb_from_compact(&compact));
    }

    #[test]
    fn expanded_geo_node_walker_decodes_linestring() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#LineString"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                    {"@list": [{"@value": 1.0_f64}, {"@value": 2.0_f64}]},
                    {"@list": [{"@value": 3.0_f64}, {"@value": 4.0_f64}]}
                ]}
            ]
        });
        let compact = json!({
            "type": "LineString",
            "coordinates": [[0.0, 0.0], [1.0, 2.0], [3.0, 4.0]]
        });
        pretty_assertions::assert_eq!(
            expanded_geo_node_to_wkb(&expanded).expect("walker decodes LineString"),
            wkb_from_compact(&compact)
        );
    }

    #[test]
    fn expanded_geo_node_walker_decodes_polygon_with_hole() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Polygon"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 4.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 4.0_f64}, {"@value": 4.0_f64}]},
                        {"@list": [{"@value": 0.0_f64}, {"@value": 4.0_f64}]},
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]}
                    ]},
                    {"@list": [
                        {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}]},
                        {"@list": [{"@value": 2.0_f64}, {"@value": 1.0_f64}]},
                        {"@list": [{"@value": 2.0_f64}, {"@value": 2.0_f64}]},
                        {"@list": [{"@value": 1.0_f64}, {"@value": 2.0_f64}]},
                        {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}]}
                    ]}
                ]}
            ]
        });
        let compact = json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
                [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0], [1.0, 1.0]]
            ]
        });
        pretty_assertions::assert_eq!(expanded_geo_node_to_wkb(&expanded).expect("walker decodes Polygon"), wkb_from_compact(&compact));
    }

    #[test]
    fn expanded_geo_node_walker_decodes_multipoint_and_multilinestring() {
        let multipoint_expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#MultiPoint"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                    {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}]}
                ]}
            ]
        });
        let multipoint_compact = json!({"type": "MultiPoint", "coordinates": [[0.0, 0.0], [1.0, 1.0]]});
        pretty_assertions::assert_eq!(
            expanded_geo_node_to_wkb(&multipoint_expanded).expect("MultiPoint"),
            wkb_from_compact(&multipoint_compact)
        );

        let multilinestring_expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#MultiLineString"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}]}
                    ]},
                    {"@list": [
                        {"@list": [{"@value": 2.0_f64}, {"@value": 2.0_f64}]},
                        {"@list": [{"@value": 3.0_f64}, {"@value": 3.0_f64}]}
                    ]}
                ]}
            ]
        });
        let multilinestring_compact = json!({
            "type": "MultiLineString",
            "coordinates": [[[0.0, 0.0], [1.0, 1.0]], [[2.0, 2.0], [3.0, 3.0]]]
        });
        pretty_assertions::assert_eq!(
            expanded_geo_node_to_wkb(&multilinestring_expanded).expect("MultiLineString"),
            wkb_from_compact(&multilinestring_compact)
        );
    }

    #[test]
    fn expanded_geo_node_walker_returns_none_for_unknown_type() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#NotAGeometry"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]}
            ]
        });
        assert!(expanded_geo_node_to_wkb(&expanded).is_none());
    }

    #[test]
    fn extract_geo_property_wkb_uses_direct_walker_for_expanded_form() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://uri.etsi.org/ngsi-ld/location": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [
                        {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}]}
                    ]
                }]
            }]
        }]);
        let compact = json!({"type": "Point", "coordinates": [16.37_f64, 48.20_f64]});
        pretty_assertions::assert_eq!(extract_location_wkb(&entity).expect("location WKB"), wkb_from_compact(&compact));
    }
}
