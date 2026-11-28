//! The clause 4.8 times a compacted Temporal Evolution writes (clause 6.3.11).

use crate::{
    differential_tests::{contexts::test_contexts, offline_engine},
    entity_compactor::EntityCompactor,
};
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    entity_system_times::EntitySystemTimes,
    temporal::{temporal_attribute::TemporalAttribute, temporal_document::TemporalDocument},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{TimeZone, Utc};
use indexmap::IndexMap;
use serde_json::json;

/// An evolution of one `speed` instance, the instance and the entity both
/// carrying their clause 4.8 times.
fn evolution_with_system_times() -> TemporalDocument {
    let speed: AttributeName = "https://uri.etsi.org/ngsi-ld/default-context/speed".parse().expect("an attribute name");
    let stamp = json!([{"@value": "2020-01-01T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]);
    let instance = json!({
        "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
        "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 5}],
        "https://uri.etsi.org/ngsi-ld/createdAt": stamp,
        "https://uri.etsi.org/ngsi-ld/modifiedAt": stamp
    });
    let instance = InstanceDocument::from_slice(&speed, &serde_json::to_vec(&instance).expect("the instance renders")).expect("the instance decodes");
    let entity = EntityDocument::new(
        Some("urn:ngsi-ld:Vehicle:1".parse().expect("an identifier")),
        vec!["https://uri.etsi.org/ngsi-ld/default-context/Vehicle".parse().expect("a type")],
        None,
        None,
        IndexMap::from([(speed, AttributeMember::Instances(vec![instance]))]),
    );
    let at = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).single();
    let times = EntitySystemTimes {
        created_at: at,
        modified_at: at,
        deleted_at: None,
    };
    TemporalDocument::new(entity, times, IndexMap::new())
}

/// Clause 6.3.11: an evolution compacted without `sysAttrs` writes neither
/// its own `createdAt` and `modifiedAt` nor those of its instances, which the
/// document carries for the merge; compacted with it, it writes both.
#[tokio::test]
async fn a_temporal_evolution_writes_its_system_times_only_under_sys_attrs() {
    let engine = offline_engine();
    let context = test_contexts().into_iter().next().expect("a test context");
    let compactor = engine.compactor(&context.spec);
    let typed = EntityCompactor::new(&compactor);
    for (system_times, written) in [(SysAttrsInclusion::Strip, false), (SysAttrsInclusion::Include, true)] {
        let entity = typed
            .temporal(evolution_with_system_times(), system_times)
            .await
            .expect("the evolution compacts");
        pretty_assertions::assert_eq!(entity.system_times.created_at.is_some(), written, "{system_times:?}");
        pretty_assertions::assert_eq!(entity.system_times.modified_at.is_some(), written, "{system_times:?}");
        let instance = entity.attrs.values().flat_map(TemporalAttribute::instances).next().expect("the instance");
        pretty_assertions::assert_eq!(instance.common().created_at.is_some(), written, "{system_times:?}");
        pretty_assertions::assert_eq!(instance.common().modified_at.is_some(), written, "{system_times:?}");
    }
}
