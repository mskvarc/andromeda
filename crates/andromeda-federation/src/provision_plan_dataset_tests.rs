//! A Delete Attribute naming a `datasetId` is forwarded only to the
//! registrations that list it or declare no `datasetId` (ETSI GS CIM 009 clause
//! 5.12): "If the request that triggered the matching includes a datasetId
//! parameter and the CSourceRegistration to be matched contains a datasetId
//! element, the CSourceRegistration should only be considered matching, if both
//! have at least one value in common."

use crate::{
    FederationService,
    fixed_registration_store::offline_service,
    operation::{ForwardingOperation, ProvisionResource},
    request::{ForwardProvisionRequest, TriggeringRequest},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::dataset_selector::DatasetSelector;
use serde_json::json;

const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:1";

/// An inclusive registration of the fixture Vehicle serving Delete Attribute,
/// declaring `datasets` when given.
fn registration(id: &str, datasets: Option<&[&str]>) -> ContextSourceRegistration {
    let mut document = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": "https://peer.example.org",
        "mode": "inclusive",
        "information": [{"entities": [{"id": ENTITY_ID, "type": "https://example.org/Vehicle"}]}],
        "operations": ["deleteAttrs"],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    });
    if let Some(datasets) = datasets {
        document["datasetId"] = json!(datasets);
    }
    serde_json::from_value(document).expect("valid registration")
}

/// The registrations a Delete Attribute of the fixture entity's `speed`
/// targeting `dataset` is sent to.
async fn receiving(service: &FederationService, dataset: Option<&DatasetSelector>) -> Vec<String> {
    let entity_id: Identifier = ENTITY_ID.parse().expect("valid identifier");
    let speed: AttributeName = "speed".parse().expect("valid attribute name");
    let context = ContextSpec::none();
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::Entities,
        entity_id: &entity_id,
        entity_type_selector: None,
        attribute_name: Some(&speed),
        dataset,
        method: reqwest::Method::DELETE,
        context: &context,
        operation: ForwardingOperation::DeleteEntityAttribute,
        path_suffix: Some("/attrs/speed"),
        query_string: None,
        triggering: TriggeringRequest { via: None, headers: None },
    };
    service
        .plan_provision(&ContextTarget::Live, &request)
        .await
        .expect("the plan is made")
        .selection
        .receiving
        .into_iter()
        .map(|registration| registration.id.to_string())
        .collect()
}

#[tokio::test]
async fn a_delete_attribute_with_a_dataset_id_reaches_only_the_registrations_listing_it_or_declaring_none() {
    let service = offline_service(vec![
        registration("urn:ngsi-ld:csr:a", Some(&["urn:ngsi-ld:Dataset:a"])),
        registration("urn:ngsi-ld:csr:b", Some(&["urn:ngsi-ld:Dataset:b"])),
        registration("urn:ngsi-ld:csr:undeclared", None),
    ]);
    let a: DatasetSelector = "urn:ngsi-ld:Dataset:a".parse().expect("a valid datasetId");
    pretty_assertions::assert_eq!(receiving(&service, Some(&a)).await, vec!["urn:ngsi-ld:csr:a", "urn:ngsi-ld:csr:undeclared"]);
    pretty_assertions::assert_eq!(
        receiving(&service, Some(&DatasetSelector::Default)).await,
        vec!["urn:ngsi-ld:csr:undeclared"],
        "`@none` is a value the registration has to list"
    );
    pretty_assertions::assert_eq!(
        receiving(&service, None).await,
        vec!["urn:ngsi-ld:csr:a", "urn:ngsi-ld:csr:b", "urn:ngsi-ld:csr:undeclared"],
        "a request naming no datasetId matches every registration"
    );
}
