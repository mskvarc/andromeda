//! Context Source Registration builders for the distributed-operations suite.
//!
//! Every registration is a valid `application/ld+json` body carrying the
//! compound test context, so a validation test starts from [`csr_basic`] and
//! mutates exactly one field to isolate a single 5.2.9 constraint, while a
//! federation test uses [`CsrBuilder`] to express mode, operations, and the
//! covered entities and attributes declaratively.

use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::{Map, Value, json};

/// A minimal valid Context Source Registration as a mutable JSON object: an
/// explicit `id`, `type`, an `endpoint`, one `RegistrationInfo` covering
/// `Vehicle` entities, and the compound `@context`. Tests mutate one field of
/// the returned object to exercise a single validation rule.
pub fn csr_basic(id: &str, endpoint: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "information": [ { "entities": [ { "type": "Vehicle" } ] } ],
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// Fluent builder for a Context Source Registration covering one entity/attribute
/// footprint under a chosen mode and operation set.
pub struct CsrBuilder {
    id: String,
    endpoint: String,
    mode: Option<String>,
    operations: Option<Vec<String>>,
    entity: Map<String, Value>,
    property_names: Vec<String>,
    relationship_names: Vec<String>,
    context_source_info: Vec<Value>,
    management: Option<Value>,
    alias: Option<String>,
}

impl CsrBuilder {
    /// Start a registration with the given id and peer endpoint, covering
    /// `Vehicle` entities by type.
    pub fn new(id: &str, endpoint: &str) -> CsrBuilder {
        let mut entity = Map::new();
        entity.insert("type".to_owned(), json!("Vehicle"));
        CsrBuilder {
            id: id.to_owned(),
            endpoint: endpoint.to_owned(),
            mode: None,
            operations: None,
            entity,
            property_names: Vec::new(),
            relationship_names: Vec::new(),
            context_source_info: Vec::new(),
            management: None,
            alias: None,
        }
    }

    /// Set the registration `mode` (`inclusive`/`exclusive`/`redirect`/`auxiliary`).
    pub fn mode(mut self, mode: &str) -> CsrBuilder {
        self.mode = Some(mode.to_owned());
        self
    }

    /// Set the advertised `operations`.
    pub fn operations(mut self, operations: &[&str]) -> CsrBuilder {
        self.operations = Some(operations.iter().map(|operation| (*operation).to_owned()).collect());
        self
    }

    /// Constrain the covered entity to an exact `id`.
    pub fn entity_id(mut self, id: &str) -> CsrBuilder {
        self.entity.insert("id".to_owned(), json!(id));
        self
    }

    /// Constrain the covered entity by `idPattern`.
    pub fn id_pattern(mut self, pattern: &str) -> CsrBuilder {
        self.entity.insert("idPattern".to_owned(), json!(pattern));
        self
    }

    /// Set the covered entity `type`.
    pub fn entity_type(mut self, entity_type: &str) -> CsrBuilder {
        self.entity.insert("type".to_owned(), json!(entity_type));
        self
    }

    /// Cover these property names in the `RegistrationInfo`.
    pub fn property_names(mut self, names: &[&str]) -> CsrBuilder {
        self.property_names = names.iter().map(|name| (*name).to_owned()).collect();
        self
    }

    /// Cover these relationship names in the `RegistrationInfo`.
    pub fn relationship_names(mut self, names: &[&str]) -> CsrBuilder {
        self.relationship_names = names.iter().map(|name| (*name).to_owned()).collect();
        self
    }

    /// Add a `contextSourceInfo` key/value pair forwarded as a header (4.3.6.6).
    pub fn context_source_info(mut self, key: &str, value: &str) -> CsrBuilder {
        self.context_source_info.push(json!({ "key": key, "value": value }));
        self
    }

    /// Set the `management` sub-object verbatim.
    pub fn management(mut self, management: Value) -> CsrBuilder {
        self.management = Some(management);
        self
    }

    /// Set the `contextSourceAlias`.
    pub fn alias(mut self, alias: &str) -> CsrBuilder {
        self.alias = Some(alias.to_owned());
        self
    }

    /// Drop the entity clause, producing a `RegistrationInfo` with only property
    /// or relationship names (used to test name-only registrations).
    pub fn without_entity(mut self) -> CsrBuilder {
        self.entity.clear();
        self
    }

    /// Materialise the registration as a JSON-LD body.
    pub fn build(self) -> Value {
        let mut info = Map::new();
        if !self.entity.is_empty() {
            info.insert("entities".to_owned(), json!([Value::Object(self.entity)]));
        }
        if !self.property_names.is_empty() {
            info.insert("propertyNames".to_owned(), json!(self.property_names));
        }
        if !self.relationship_names.is_empty() {
            info.insert("relationshipNames".to_owned(), json!(self.relationship_names));
        }
        let mut body = Map::new();
        body.insert("id".to_owned(), json!(self.id));
        body.insert("type".to_owned(), json!("ContextSourceRegistration"));
        body.insert("endpoint".to_owned(), json!(self.endpoint));
        body.insert("information".to_owned(), json!([Value::Object(info)]));
        if let Some(mode) = self.mode {
            body.insert("mode".to_owned(), json!(mode));
        }
        if let Some(operations) = self.operations {
            body.insert("operations".to_owned(), json!(operations));
        }
        if !self.context_source_info.is_empty() {
            body.insert("contextSourceInfo".to_owned(), json!(self.context_source_info));
        }
        if let Some(management) = self.management {
            body.insert("management".to_owned(), management);
        }
        if let Some(alias) = self.alias {
            body.insert("contextSourceAlias".to_owned(), json!(alias));
        }
        body.insert("@context".to_owned(), json!(COMPOUND_CONTEXT.as_str()));
        Value::Object(body)
    }
}
