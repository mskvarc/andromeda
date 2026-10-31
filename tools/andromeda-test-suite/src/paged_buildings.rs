//! Four Buildings of one type for walking a cursor over a query (clause 5.5.9.1): `Building:{ns}-0000` to `-0003`,
//! `name` b0 to b3, `height` 10 to 40, and `location` Points at [0, 0], [0, 0.001], [0, 0.002] and [50, 50], so a
//! filter on `height` above 5 keeps all four and a near query within 1,000 m of [0, 0] keeps the first three.

use crate::{
    broker::BrokerClient,
    namespace::Namespace,
    raw_call::{call, target},
};
use bytes::Bytes;
use googletest::Result as TestResult;
use http::{Method, Response};
use ngsi_ld::model::{
    Entity,
    EntityId,
    EntityTypeName,
    geometry::{Position, Shape},
};

/// The created Buildings.
#[derive(Debug)]
pub struct PagedBuildings {
    /// The type every Building has.
    pub entity_type: EntityTypeName,
    /// The identifiers, in identifier order.
    pub ids: [EntityId; 4],
}

/// The `location` of each Building, as longitude and latitude.
const LOCATIONS: [(f64, f64); 4] = [(0.0, 0.0), (0.0, 0.001), (0.0, 0.002), (50.0, 50.0)];

impl PagedBuildings {
    /// Creates the four Buildings of `namespace` on `broker`.
    ///
    /// # Errors
    ///
    /// Returns the error of an identifier, a position, an entity or a create that fails.
    pub async fn create(broker: &BrokerClient, namespace: &Namespace) -> TestResult<PagedBuildings> {
        let entity_type = namespace.entity_type("Building")?;
        let ids = [
            namespace.urn("Building", "0000")?,
            namespace.urn("Building", "0001")?,
            namespace.urn("Building", "0002")?,
            namespace.urn("Building", "0003")?,
        ];
        for (number, (id, (longitude, latitude))) in ids.iter().zip(LOCATIONS).enumerate() {
            let height = 10 * (number + 1);
            let building = Entity::builder()
                .id(id.clone())
                .entity_type(entity_type.clone())
                .property("name", format!("b{number}"))
                .property("height", u32::try_from(height)?)
                .geo_property("location", Shape::Point(Position::try_new(longitude, latitude)?))
                .build()?;
            broker.entities().create(&building).send().await?;
        }
        Ok(PagedBuildings { entity_type, ids })
    }

    /// The first page of Query Entities over the Buildings with `local=true`, `limit` and `parameters` after them,
    /// as JSON-LD.
    ///
    /// # Errors
    ///
    /// Returns the error of a call that cannot be made.
    pub async fn first_page(&self, broker: &BrokerClient, limit: &str, parameters: &[(&str, &str)]) -> TestResult<Response<Bytes>> {
        // `local` is a parameter the query model does not write, so the query is sent as written.
        let entity_type = self.entity_type.to_string();
        let mut query = vec![("type", entity_type.as_str()), ("local", "true"), ("limit", limit)];
        query.extend_from_slice(parameters);
        Ok(call(broker, Method::GET, &target("entities", &query), &[("Accept", "application/ld+json")], None).await?)
    }
}
