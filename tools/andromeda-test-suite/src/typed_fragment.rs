//! An Add Attributes to Temporal Evolution request whose Fragment also names an entity `type` (clause 5.4), which the
//! typed operation never writes: the instances are written by the model and the `type` member is set on the document.

use crate::{
    broker::BrokerClient,
    unchecked_document::{UncheckedDocumentError, from_rendered},
};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{UncheckedRequest, operation::AddTemporalAttributes},
    model::{EntityId, EntityTypeName, Input, Lossless, evolution::TemporalAttributes},
};

/// Why the request could not be built.
#[derive(Debug, thiserror::Error)]
pub enum TypedFragmentError {
    /// The rendered instances are not a document the request can carry.
    #[error(transparent)]
    Document(#[from] UncheckedDocumentError),
    /// The client refused to build the request.
    #[error("the request could not be built")]
    Request(#[source] Box<ClientError<ReqwestError>>),
}

/// The Add Attributes request of `instances` to the temporal evolution of `id`, its Fragment naming `entity_type`.
///
/// # Errors
///
/// Returns [`TypedFragmentError`] when the document or the request cannot be built.
pub fn naming_type(
    client: &BrokerClient,
    id: &EntityId,
    instances: &TemporalAttributes<Input>,
    entity_type: &EntityTypeName,
) -> Result<UncheckedRequest, TypedFragmentError> {
    let document = from_rendered(&instances.render(Lossless::Normalized))?.set_text("type", entity_type.as_str());
    let request = client
        .unchecked(&AddTemporalAttributes::builder().id(id).attributes(instances).build())
        .map_err(|error| TypedFragmentError::Request(Box::new(error)))?;
    Ok(request.document(&document))
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, typed_fragment::naming_type};
    use googletest::prelude::*;
    use ngsi_ld::model::{EntityId, EntityTypeName, Property, evolution::TemporalEntity};
    use url::Url;

    #[gtest]
    fn the_fragment_carries_the_instances_and_the_type() -> Result<()> {
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let id = "urn:ngsi-ld:Vehicle:1".parse::<EntityId>()?;
        let vehicle_type = "Vehicle".parse::<EntityTypeName>()?;
        let car_type = "Car".parse::<EntityTypeName>()?;
        let instances = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type)
            .property("speed", Property::builder(80).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?
            .attributes;
        let request = naming_type(&client, &id, &instances, &car_type)?;
        let body = String::from_utf8_lossy(request.as_request().body()).into_owned();
        verify_that!(body, all!(contains_substring(r#""type":"Car""#), contains_substring(r#""speed""#)))
    }
}
