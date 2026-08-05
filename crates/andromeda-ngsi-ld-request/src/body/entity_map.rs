//! HTTP request model types for entity map query endpoints.

use serde::Deserialize;
use serde_json::Value;

/// Request body for POST-based entity map creation (section 5.14.4, 6.34.3.2).
#[derive(Debug, Deserialize)]
pub struct QueryRequestBody {
    /// Entity selectors constraining the query.
    pub entities: Option<Vec<EntitySelector>>,
    /// Attribute names to include.
    pub attrs: Option<Vec<String>>,
    /// NGSI-LD query expression.
    pub q: Option<String>,
    /// Geo-relationship for geo-queries.
    pub georel: Option<String>,
    /// Geometry type for geo-queries.
    pub geometry: Option<String>,
    /// Coordinates for geo-queries.
    pub coordinates: Option<String>,
    /// Geo-property for geo-queries.
    pub geoproperty: Option<String>,
    /// Scope query expression.
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<String>,
    /// Requested entity map lifetime as ISO 8601 duration.
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
}

/// A single entity selector within a query request body.
#[derive(Debug, Deserialize)]
pub struct EntitySelector {
    /// Entity IDs to match.
    pub id: Option<Vec<String>>,
    /// ID pattern to match.
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    /// Entity type to match.
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

/// Request body for PATCH entity map (section 5.14.2).
#[derive(Debug, Deserialize)]
pub struct UpdateEntityMapBody {
    /// New expiration time.
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<String>,
    /// Output-only field -- accepted but ignored (section 5.14.2.4).
    #[serde(rename = "entityMap")]
    #[allow(dead_code)]
    pub entity_map: Option<Value>,
    /// Output-only field -- accepted but ignored (section 5.14.2.4).
    #[serde(rename = "linkedMaps")]
    #[allow(dead_code)]
    pub linked_maps: Option<Value>,
    /// Output-only field -- accepted but ignored (section 5.14.2.4).
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub type_: Option<String>,
    /// Output-only field -- accepted but ignored (section 5.14.2.4).
    #[allow(dead_code)]
    pub id: Option<String>,
}

/// Convert a `QueryRequestBody` into `EntityQueryParameters` via URL-encoded round-trip.
///
/// Maps the JSON body fields into the same parameter format used by GET query endpoints.
pub fn query_body_to_params(body: &QueryRequestBody) -> Result<crate::query::entity::EntityQueryParameters, serde_urlencoded::de::Error> {
    let (id, entity_type, id_pattern) = if let Some(entities) = &body.entities {
        let ids: Vec<String> = entities.iter().filter_map(|e| e.id.clone()).flatten().collect();
        let types: Vec<String> = entities.iter().filter_map(|e| e.type_.clone()).collect();
        (
            if ids.is_empty() { None } else { Some(ids.join(",")) },
            if types.is_empty() { None } else { Some(types.join(",")) },
            entities.iter().filter_map(|e| e.id_pattern.clone()).next(),
        )
    } else {
        (None, None, None)
    };

    // Use serde_urlencoded to construct parameters matching EntityQueryParameters
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(ref id) = id {
        pairs.push(("id".to_string(), id.clone()));
    }
    if let Some(ref t) = entity_type {
        pairs.push(("type".to_string(), t.clone()));
    }
    if let Some(ref p) = id_pattern {
        pairs.push(("idPattern".to_string(), p.clone()));
    }
    if let Some(ref attrs) = body.attrs {
        pairs.push(("attrs".to_string(), attrs.join(",")));
    }
    if let Some(ref q) = body.q {
        pairs.push(("q".to_string(), q.clone()));
    }
    if let Some(ref georel) = body.georel {
        pairs.push(("georel".to_string(), georel.clone()));
    }
    if let Some(ref geometry) = body.geometry {
        pairs.push(("geometry".to_string(), geometry.clone()));
    }
    if let Some(ref coordinates) = body.coordinates {
        pairs.push(("coordinates".to_string(), coordinates.clone()));
    }
    if let Some(ref geoproperty) = body.geoproperty {
        pairs.push(("geoproperty".to_string(), geoproperty.clone()));
    }
    if let Some(ref scope_q) = body.scope_q {
        pairs.push(("scopeQ".to_string(), scope_q.clone()));
    }
    // Always add local=true since entity map creation is always local
    pairs.push(("local".to_string(), "true".to_string()));

    let qs = serde_urlencoded::to_string(&pairs).unwrap_or_default();
    serde_urlencoded::from_str(&qs).or_else(|_| {
        // Fallback: construct with defaults via minimal query string
        serde_urlencoded::from_str("local=true")
    })
}
