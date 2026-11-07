//! Merging the discovery answers of context sources with the local one
//! (clauses 5.7.5 to 5.7.10, 6.3.17).
//!
//! Every answer is decoded into the typed model of the resource asked for; an
//! answer that does not decode contributes nothing and a clause 6.3.17
//! warning naming its registration. Names merge by the Fully Qualified Name
//! they stand for: a record's `id` is that name already (Tables 5.2.25-1,
//! 5.2.26-1, 5.2.28-1), and a name a list carries is expanded under the
//! context its source spelled it in. Every merged name is then spelled in the
//! client's terms by the one compaction of the request context, so two sources
//! spelling one type differently answer one type.

use crate::{FederationService, discovery_answer_failure::DiscoveryAnswerFailure, error::FederationError, outcome::ForwardedDiscoveryResult};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::{
    attribute_list::{AttributeDetail, AttributeInfo, AttributeList},
    entity_type_list::{EntityType, EntityTypeInfo, EntityTypeList},
};
use iri_rs::IriBuf;
use serde::de::DeserializeOwned;
use std::{mem, str::FromStr};
use tracing::warn;

/// One discovery answer, in the typed model of the resource it is.
#[derive(Debug)]
pub enum Discovered {
    /// Clause 5.2.24 `EntityTypeList`.
    Types(EntityTypeList),
    /// Clause 5.2.25 `EntityType` records, `details=true`.
    TypeDetails(Vec<EntityType>),
    /// Clause 5.2.26 `EntityTypeInfo`.
    TypeInfo(EntityTypeInfo),
    /// Clause 5.2.27 `AttributeList`.
    Attributes(AttributeList),
    /// Clause 5.2.28 `Attribute` records, `details=true`.
    AttributeDetails(Vec<AttributeInfo>),
    /// Clause 5.2.28 `Attribute`.
    AttributeInfo(AttributeInfo),
}

/// Collect protocol warnings from all forwarded discovery outcomes in encounter order.
#[must_use]
pub fn collect_discovery_warnings(results: &[ForwardedDiscoveryResult]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|result| result.warnings.iter().cloned()).collect()
}

/// The answers a merge reads, decoded, beside the warnings of those that do
/// not decode.
struct DecodedAnswers<T> {
    /// Each decoded answer with the context its source spelled it in.
    answers: Vec<(T, ContextSpec)>,
    /// One clause 6.3.17 warning per answer that does not decode.
    warnings: Vec<NgsiLdWarning>,
}

/// Decode every answer `remote` carries as a `T`.
fn decoded<T: DeserializeOwned>(remote: Vec<ForwardedDiscoveryResult>) -> DecodedAnswers<T> {
    let mut answers = Vec::with_capacity(remote.len());
    let mut warnings = Vec::new();
    for result in remote {
        let Some(body) = result.body else {
            continue;
        };
        match serde_json::from_slice::<T>(&body) {
            Ok(answer) => answers.push((answer, result.naming)),
            Err(error) => {
                warn!(registration = ?result.registration_id, %error, "a context source answered discovery with a malformed body");
                warnings.extend(
                    result
                        .registration_id
                        .as_ref()
                        .map(|registration| DiscoveryAnswerFailure::Malformed { registration, error }.into_warning()),
                );
            }
        }
    }
    DecodedAnswers { answers, warnings }
}

impl FederationService {
    /// Merge the local discovery answer with what the context sources
    /// answered, every name spelled under `context`.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when `context` does not resolve or
    /// process.
    pub async fn merge_discovered(
        &self,
        local: Discovered,
        remote: Vec<ForwardedDiscoveryResult>,
        context: &ContextSpec,
    ) -> Result<(Discovered, Vec<NgsiLdWarning>), FederationError> {
        if remote.iter().all(|result| result.body.is_none()) {
            return Ok((local, Vec::new()));
        }
        let names = NameCompaction::new(self.engine.compactor(context).processed_context().await?);
        Ok(match local {
            Discovered::Types(mut list) => {
                let DecodedAnswers { answers, mut warnings } = decoded::<EntityTypeList>(remote);
                for (answer, naming) in answers {
                    for name in answer.type_list {
                        match self.respelled_type(&name, &naming, &names).await {
                            Some(name) if !list.type_list.contains(&name) => list.type_list.push(name),
                            Some(_) => {}
                            None => warnings.push(DiscoveryAnswerFailure::UnexpandableType { name: &name }.into_warning()),
                        }
                    }
                }
                (Discovered::Types(list), warnings)
            }
            Discovered::Attributes(mut list) => {
                let DecodedAnswers { answers, mut warnings } = decoded::<AttributeList>(remote);
                for (answer, naming) in answers {
                    for name in answer.attribute_list {
                        match self.respelled_attribute(&name, &naming, &names).await {
                            Some(name) if !list.attribute_list.contains(&name) => list.attribute_list.push(name),
                            Some(_) => {}
                            None => warnings.push(DiscoveryAnswerFailure::UnexpandableAttribute { name: &name }.into_warning()),
                        }
                    }
                }
                (Discovered::Attributes(list), warnings)
            }
            Discovered::TypeDetails(mut types) => {
                let DecodedAnswers { answers, mut warnings } = decoded::<Vec<EntityType>>(remote);
                for (answer, naming) in answers {
                    for mut record in answer {
                        let mut attribute_names = Vec::with_capacity(record.attribute_names.len());
                        for name in mem::take(&mut record.attribute_names) {
                            match self.respelled_attribute(&name, &naming, &names).await {
                                Some(name) => attribute_names.push(name),
                                None => warnings.push(DiscoveryAnswerFailure::UnexpandableAttribute { name: &name }.into_warning()),
                            }
                        }
                        record.attribute_names = attribute_names;
                        merge_entity_type(&mut types, record, &names);
                    }
                }
                (Discovered::TypeDetails(types), warnings)
            }
            Discovered::TypeInfo(mut info) => {
                let DecodedAnswers { answers, warnings } = decoded::<EntityTypeInfo>(remote);
                for (answer, _) in answers.into_iter().filter(|(answer, _)| answer.id == info.id) {
                    // The counts of every holder are summed; an entity split over several holders is counted once per holder, which no holder's answer can tell apart.
                    info.entity_count = info.entity_count.saturating_add(answer.entity_count);
                    for detail in answer.attribute_details {
                        merge_attribute_detail(&mut info.attribute_details, detail, &names);
                    }
                }
                (Discovered::TypeInfo(info), warnings)
            }
            Discovered::AttributeDetails(mut attributes) => {
                let DecodedAnswers { answers, mut warnings } = decoded::<Vec<AttributeInfo>>(remote);
                for (answer, naming) in answers {
                    for record in answer {
                        let record = self.respelled_attribute_info(record, &naming, &names, &mut warnings).await;
                        match attributes.iter_mut().find(|held| held.id == record.id) {
                            Some(held) => merge_attribute_info(held, record),
                            None => attributes.push(record),
                        }
                    }
                }
                (Discovered::AttributeDetails(attributes), warnings)
            }
            Discovered::AttributeInfo(mut info) => {
                let DecodedAnswers { answers, mut warnings } = decoded::<AttributeInfo>(remote);
                for (answer, naming) in answers {
                    // A source answering for another attribute says nothing
                    // about this one.
                    if answer.id != info.id {
                        continue;
                    }
                    let record = self.respelled_attribute_info(answer, &naming, &names, &mut warnings).await;
                    merge_attribute_info(&mut info, record);
                }
                (Discovered::AttributeInfo(info), warnings)
            }
        })
    }

    /// A type name a source spelled under `naming`, spelled in the client's
    /// terms; `None` when it does not expand.
    pub(crate) async fn respelled_type(&self, name: &EntityTypeName, naming: &ContextSpec, names: &NameCompaction) -> Option<EntityTypeName> {
        let prepared = self.engine.prepare_term_context(naming);
        let iri = self.engine.expand_type(name.as_str(), &prepared).await.ok()?;
        client_type_name(&iri, names)
    }

    /// An attribute name a source spelled under `naming`, spelled in the
    /// client's terms; `None` when it does not expand.
    pub(crate) async fn respelled_attribute(&self, name: &AttributeName, naming: &ContextSpec, names: &NameCompaction) -> Option<AttributeName> {
        let prepared = self.engine.prepare_term_context(naming);
        let iri = self.engine.expand_term(name.as_str(), &prepared).await.ok()?;
        client_attribute_name(iri.as_str(), names)
    }

    /// An `Attribute` record a source spelled under `naming`, its names
    /// spelled in the client's terms: the attribute's from its FQN `id`, each
    /// type name expanded under `naming` first.
    async fn respelled_attribute_info(
        &self,
        mut record: AttributeInfo,
        naming: &ContextSpec,
        names: &NameCompaction,
        warnings: &mut Vec<NgsiLdWarning>,
    ) -> AttributeInfo {
        if let Some(name) = client_attribute_name(record.id.as_str(), names) {
            record.attribute_name = name;
        }
        let mut type_names = Vec::with_capacity(record.type_names.len());
        for name in mem::take(&mut record.type_names) {
            match self.respelled_type(&name, naming, names).await {
                Some(name) => type_names.push(name),
                None => warnings.push(DiscoveryAnswerFailure::UnexpandableType { name: &name }.into_warning()),
            }
        }
        record.type_names = type_names;
        record
    }
}

/// The client's term for the type `iri`, or the IRI itself where the term is
/// no type name; `None` when neither is.
fn client_type_name(iri: &IriBuf, names: &NameCompaction) -> Option<EntityTypeName> {
    EntityTypeName::from_str(&names.compact_type_iri(iri.as_str()))
        .or_else(|_| EntityTypeName::from_str(iri.as_str()))
        .ok()
}

/// The client's term for the attribute `iri`, or the IRI itself where the
/// term is no attribute name; `None` when neither is.
fn client_attribute_name(iri: &str, names: &NameCompaction) -> Option<AttributeName> {
    AttributeName::from_str(&names.compact_attr_iri(iri))
        .or_else(|_| AttributeName::from_str(iri))
        .ok()
}

/// Add every member of `values` that `held` lacks, in order.
fn union<T: PartialEq>(held: &mut Vec<T>, values: impl IntoIterator<Item = T>) {
    for value in values {
        if !held.contains(&value) {
            held.push(value);
        }
    }
}

/// Fold an `EntityType` record a source answered into the records held, keyed by its Fully Qualified Name.
fn merge_entity_type(types: &mut Vec<EntityType>, mut record: EntityType, names: &NameCompaction) {
    // A source's attribute names are its own terms; the record's `id` is the
    // type's FQN, so the type's name is spelled from it.
    if let Some(type_name) = client_type_name(&record.id, names) {
        record.type_name = type_name;
    }
    match types.iter_mut().find(|held| held.id == record.id) {
        Some(held) => union(&mut held.attribute_names, record.attribute_names),
        None => types.push(record),
    }
}

/// Fold an attribute detail a source answered into the details held, keyed by the attribute's FQN, their families unioned.
fn merge_attribute_detail(details: &mut Vec<AttributeDetail>, mut detail: AttributeDetail, names: &NameCompaction) {
    if let Some(held) = details.iter_mut().find(|held| held.id == detail.id) {
        union(&mut held.attribute_types, detail.attribute_types);
        return;
    }
    if let Some(name) = client_attribute_name(detail.id.as_str(), names) {
        detail.attribute_name = name;
    }
    details.push(detail);
}

/// Fold an `Attribute` record a source answered for the same attribute, already spelled in the client's terms, into the one held: families and type names unioned, counts summed.
fn merge_attribute_info(held: &mut AttributeInfo, record: AttributeInfo) {
    union::<AttributeType>(&mut held.attribute_types, record.attribute_types);
    union(&mut held.type_names, record.type_names);
    held.attribute_count = match (held.attribute_count, record.attribute_count) {
        (Some(own), Some(theirs)) => Some(own.saturating_add(theirs)),
        (Some(count), None) | (None, Some(count)) => Some(count),
        (None, None) => None,
    };
}

#[cfg(test)]
mod tests {
    use crate::{discovery_result::Discovered, fixed_registration_store::offline_service, outcome::ForwardedDiscoveryResult};
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use serde_json::{Value, json};
    use std::sync::Arc;

    const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";

    fn answer(body: &Value, naming: ContextSpec) -> ForwardedDiscoveryResult {
        ForwardedDiscoveryResult {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a URI")),
            body: Some(Arc::from(body.to_string().into_bytes())),
            naming,
            warnings: Vec::new(),
        }
    }

    async fn merged(local: Value, remote: Vec<ForwardedDiscoveryResult>, wrap: fn(Value) -> Discovered) -> (Value, Vec<NgsiLdWarning>) {
        let (discovered, warnings) = offline_service(Vec::new())
            .merge_discovered(wrap(local), remote, &ContextSpec::core_only())
            .await
            .expect("merges");
        (written(discovered), warnings)
    }

    /// The answer as the JSON the response body is written as.
    fn written(discovered: Discovered) -> Value {
        match discovered {
            Discovered::Types(list) => serde_json::to_value(list),
            Discovered::TypeDetails(types) => serde_json::to_value(types),
            Discovered::TypeInfo(info) => serde_json::to_value(info),
            Discovered::Attributes(list) => serde_json::to_value(list),
            Discovered::AttributeDetails(attributes) => serde_json::to_value(attributes),
            Discovered::AttributeInfo(info) => serde_json::to_value(info),
        }
        .expect("a typed answer serialises")
    }

    fn type_info(body: Value) -> Discovered {
        Discovered::TypeInfo(serde_json::from_value(body).expect("an EntityTypeInfo"))
    }

    fn attribute_info(body: Value) -> Discovered {
        Discovered::AttributeInfo(serde_json::from_value(body).expect("an Attribute"))
    }

    fn types(body: Value) -> Discovered {
        Discovered::Types(serde_json::from_value(body).expect("an EntityTypeList"))
    }

    /// F-D1: a spec-shaped remote `EntityTypeInfo` contributes its attribute
    /// details, their families unioned with the local ones.
    #[tokio::test]
    async fn type_info_merges_attribute_details() {
        let local = json!({"id": VEHICLE, "type": "EntityTypeInfo", "typeName": "Vehicle", "entityCount": 3,
            "attributeDetails": [{"id": SPEED, "type": "Attribute", "attributeName": "speed", "attributeTypes": ["Property"]}]});
        let remote = json!({"id": VEHICLE, "type": "EntityTypeInfo", "typeName": "Vehicle", "entityCount": 2,
        "attributeDetails": [
            {"id": SPEED, "type": "Attribute", "attributeName": "speed", "attributeTypes": ["GeoProperty"]},
            {"id": "https://uri.etsi.org/ngsi-ld/default-context/name", "type": "Attribute", "attributeName": "name", "attributeTypes": ["Property"]}
        ]});
        let (merged, _) = merged(local, vec![answer(&remote, ContextSpec::core_only())], type_info).await;
        pretty_assertions::assert_eq!(merged["attributeDetails"][0]["attributeTypes"], json!(["Property", "GeoProperty"]));
        pretty_assertions::assert_eq!(merged["attributeDetails"][1]["attributeName"], json!("name"));
        pretty_assertions::assert_eq!(merged["entityCount"], json!(5));
    }

    /// F-D2: a remote `Attribute` record contributes its families and count.
    #[tokio::test]
    async fn attribute_info_merges_types_and_counts() {
        let local =
            json!({"id": SPEED, "type": "Attribute", "attributeName": "speed", "attributeTypes": ["Property"], "attributeCount": 2, "typeNames": ["Vehicle"]});
        let remote =
            json!({"id": SPEED, "type": "Attribute", "attributeName": "speed", "attributeTypes": ["Relationship"], "attributeCount": 1, "typeNames": ["Car"]});
        let (merged, _) = merged(local, vec![answer(&remote, ContextSpec::core_only())], attribute_info).await;
        let families: Vec<AttributeType> = serde_json::from_value(merged["attributeTypes"].clone()).expect("families"); // clone: the assertion reads the member out of the answer
        pretty_assertions::assert_eq!(families, vec![AttributeType::Property, AttributeType::Relationship]);
        pretty_assertions::assert_eq!(merged["attributeCount"], json!(3));
        pretty_assertions::assert_eq!(merged["typeNames"], json!(["Vehicle", "Car"]));
    }

    /// F-D4: a malformed discovery answer contributes nothing and a warning
    /// naming its registration.
    #[tokio::test]
    async fn a_malformed_discovery_answer_is_reported() {
        let local = json!({"id": "urn:ngsi-ld:EntityTypeList:1", "type": "EntityTypeList", "typeList": ["Vehicle"]});
        let (merged, warnings) = merged(local, vec![answer(&json!({"typeList": 5}), ContextSpec::core_only())], types).await;
        pretty_assertions::assert_eq!(merged["typeList"], json!(["Vehicle"]));
        let [warning] = warnings.as_slice() else {
            panic!("one warning: {warnings:?}");
        };
        pretty_assertions::assert_eq!(warning.code(), WarnCode::RevalidationFailed);
        assert!(warning.text().as_str().contains("urn:ngsi-ld:ContextSourceRegistration:1"), "{warning}");
    }

    /// A type a source spells under its own context merges with the local
    /// spelling of the same IRI.
    #[tokio::test]
    async fn types_spelled_differently_by_a_source_merge() {
        let local = json!({"id": "urn:ngsi-ld:EntityTypeList:1", "type": "EntityTypeList", "typeList": ["Vehicle"]});
        let naming = ContextSpec::from_body(JsonLiteral::admit(&format!(r#"{{"Veh":"{VEHICLE}"}}"#)).expect("a literal"));
        let remote = json!({"id": "urn:ngsi-ld:EntityTypeList:2", "type": "EntityTypeList", "typeList": ["Veh", "Drone"]});
        let (merged, warnings) = merged(local, vec![answer(&remote, naming)], types).await;
        pretty_assertions::assert_eq!(merged["typeList"], json!(["Vehicle", "Drone"]));
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}
