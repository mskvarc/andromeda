//! A Snapshot large enough that a clone of it copies for several seconds, so a test can act on the clone or its
//! source while the copy runs (clause 5.16.2.4): 40,000 Devices of one type, each with eight Properties and a
//! `location` Point, captured by one Snapshot.

use crate::{broker::BrokerClient, namespace::Namespace, outcome::Done, snapshot_capture::await_status};
use googletest::Result as TestResult;
use ngsi_ld::{
    model::{
        Entity,
        Input,
        geometry::{Position, Shape},
    },
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// The number of Devices the Snapshot captures.
const DEVICES: u32 = 40_000;

/// The Properties every Device holds, each valued with the Device's number.
const PROPERTIES: [&str; 8] = ["temperature", "humidity", "pressure", "voltage", "current", "power", "energy", "frequency"];

/// The number of Devices one batch creates.
const CHUNK: usize = 1_000;

/// Creates the Devices of `namespace` on `tenant` and the Snapshot `urn:ngsi-ld:Snapshot:{ns}-source` capturing them,
/// and waits until it reports `success`.
///
/// # Errors
///
/// Returns the error of an identifier, a position, an entity, a batch, the Snapshot or a wait that fails.
pub async fn create(tenant: &BrokerClient, namespace: &Namespace) -> TestResult<SnapshotId> {
    let device_type = namespace.entity_type("Device")?;
    let devices = (1..=DEVICES)
        .map(|number| -> TestResult<Entity<Input>> {
            let longitude = f64::from(number % 360) - 180.0;
            let device = Entity::builder()
                .id(namespace.urn("Device", number)?)
                .entity_type(device_type.clone())
                .geo_property("location", Shape::Point(Position::try_new(longitude, 0.0)?));
            Ok(PROPERTIES.iter().fold(device, |device, name| device.property(*name, number)).build()?)
        })
        .collect::<TestResult<Vec<_>>>()?;
    for chunk in devices.chunks(CHUNK) {
        tenant.entities().batch_create(chunk).send().await?.done()?;
    }
    let source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-source").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(source_id.clone())
        .query(QueryDocument::builder().entities([device_type.as_str()]).build()?)
        .build()?;
    tenant.snapshots().create(&snapshot).send().await?;
    await_status(tenant, &source_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    Ok(source_id)
}
