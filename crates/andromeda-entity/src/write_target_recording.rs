//! Every entity write holds its target to the `type` selector of the request
//! (clauses 5.6.2.4 to 5.6.6.4, 5.6.17.4, 5.6.18.4 and 5.6.19.4): "no existing
//! Entity whose id (URI), and where specified type, is equivalent" is a
//! `ResourceNotFound`, proven against the shared recording harness whose one
//! stored entity is a Building.

use crate::{
    EntityService,
    admission_fixture::{admitted_attribute, admitted_context, admitted_entity, admitted_written},
    deletion_outline::DeletionWatchers,
    deletion_outline_fixture::ProbeAnswer,
    error::EntityError,
    merge_options::MergeOptions,
    notification_recording_harness::{CachedSubscription, attribute_name, core_context, entity_uri, live_target, probing_recording_service, recording_service},
    write_kind::{AppendAttributes, MergeEntity, PartialUpdateAttribute, ReplaceAttribute, ReplaceEntity, UpdateAttributes},
    write_target::WriteTarget,
};
use andromeda_ngsi_ld_entity::operation::selection::{DatasetSelection, OverwritePolicy};
use andromeda_ngsi_ld_model::{attribute::member_reading::MemberReading, entity::EntityPatch};
use andromeda_ngsi_ld_query::type_selection::grammar::{TypeExpr, TypeExprRaw};
use serde_json::{Value, json};

/// The selector `raw` expands to under the core context.
async fn expanded(service: &EntityService, raw: &str) -> TypeExpr {
    let raw: TypeExprRaw = raw.parse().expect("a clause 4.17 selector");
    service
        .expand_write_selector(Some(&raw), &core_context())
        .await
        .expect("the selector expands")
        .expect("a selector was given")
}

/// Build an [`EntityPatch`] from a compact fixture payload.
fn patch<M: MemberReading>(payload: Value) -> EntityPatch<M> {
    serde_json::from_value(payload).expect("the fixture patch is a valid entity patch")
}

/// The selectors the stored Building satisfies.
const INSIDE: [&str; 2] = ["Building", "Vehicle|Building"];

/// The selector the stored Building does not satisfy.
const OUTSIDE: &str = "Vehicle";

/// Generate, for each operation, the test that a write outside the selector is
/// a `ResourceNotFound` recording nothing and the test that a write inside it
/// lands. `$write` is the operation's write against `$service` and `$entity`.
macro_rules! selector_tests {
    ($($operation:ident => |$service:ident, $entity:ident| $write:expr),+ $(,)?) => {
        $(
            mod $operation {
                use super::*;

                /// Clause 5.6.x.4: an entity outside the selector is not the target.
                #[tokio::test]
                async fn a_write_to_an_entity_outside_the_type_selector_is_not_found() {
                    let ($service, recorded) = recording_service().await;
                    let selector = expanded(&$service, OUTSIDE).await;
                    let id = entity_uri();
                    let $entity = WriteTarget::new(&id, Some(&selector));
                    let $service = &$service;

                    let error = $write.await.expect_err("the Building is outside the Vehicle selector");

                    assert!(matches!(error, EntityError::NotFound { .. }), "{error:?}");
                    assert!(recorded.lock().expect("recording mutex is not poisoned").is_empty(), "nothing is committed");
                }

                /// Clause 4.17: an entity satisfying the selector is the target.
                #[tokio::test]
                async fn a_write_inside_the_type_selector_proceeds() {
                    for raw in INSIDE {
                        let ($service, recorded) = recording_service().await;
                        let selector = expanded(&$service, raw).await;
                        let id = entity_uri();
                        let $entity = WriteTarget::new(&id, Some(&selector));
                        let $service = &$service;

                        $write.await.unwrap_or_else(|error| panic!("{raw}: {error:?}"));

                        assert_eq!(recorded.lock().expect("recording mutex is not poisoned").len(), 1, "{raw}: the write lands");
                    }
                }
            }
        )+
    };
}

selector_tests! {
    append_attributes => |service, entity| async {
        let admitted = admitted_written::<AppendAttributes>(service, json!({"name": {"type": "Property", "value": "lobby"}}), &core_context()).await;
        service.append_attrs(&live_target(), entity, admitted, OverwritePolicy::Overwrite).await.map(|_appended| ())
    },
    update_attributes => |service, entity| async {
        let admitted = admitted_written::<UpdateAttributes>(service, json!({"category": {"type": "Property", "value": "retail"}}), &core_context()).await;
        service.update_attrs(&live_target(), entity, admitted).await.map(|_updated| ())
    },
    partial_attribute_update => |service, entity| async {
        let category = attribute_name("category");
        let admitted =
            admitted_attribute::<PartialUpdateAttribute>(service, &category, json!({"type": "Property", "value": "retail"}), &core_context()).await;
        service.partial_update_attr(&live_target(), entity, admitted).await
    },
    replace_attribute => |service, entity| async {
        let category = attribute_name("category");
        let admitted = admitted_attribute::<ReplaceAttribute>(service, &category, json!({"type": "Property", "value": "retail"}), &core_context()).await;
        service.replace_attr(&live_target(), entity, admitted).await
    },
    delete_attribute => |service, entity| async {
        service
            .delete_attr(&live_target(), entity, &attribute_name("category"), DatasetSelection::Default, &core_context())
            .await
    },
    replace_entity => |service, entity| async {
        let admitted = admitted_entity::<ReplaceEntity>(
            service,
            &patch(json!({"type": "Building", "name": {"type": "Property", "value": "lobby"}})),
            &core_context(),
        )
        .await;
        service.replace_entity(&live_target(), entity, admitted).await
    },
    merge_entity => |service, entity| async {
        let fragment = patch(json!({"category": {"type": "Property", "value": "retail"}}));
        let admitted = admitted_entity::<MergeEntity>(service, &fragment, &core_context()).await;
        let context = admitted_context(service, &core_context()).await;
        service.merge(&live_target(), entity, &fragment, admitted, &context, MergeOptions::default()).await
    },
}

/// Delete the stored Building under the selector `raw` through a service whose
/// deletion-watcher probe answers `watchers`, returning the outcome and how
/// many writes were committed.
async fn deleted_under(watchers: DeletionWatchers, raw: &str) -> (Result<(), EntityError>, usize) {
    let observed = probing_recording_service(ProbeAnswer::Answers(watchers), CachedSubscription::None).await;
    let selector = expanded(&observed.service, raw).await;
    let id = entity_uri();
    let outcome = observed.service.delete(&live_target(), WriteTarget::new(&id, Some(&selector))).await;
    let committed = observed.recorded.lock().expect("recording mutex is not poisoned").len();
    (outcome, committed)
}

/// Clause 5.6.6.4: a deletion owed no notification is outlined, and an entity
/// outside the selector is as absent as one that does not exist.
#[tokio::test]
async fn a_delete_of_an_outlined_entity_outside_the_type_selector_is_not_found() {
    let (outcome, committed) = deleted_under(DeletionWatchers::None, OUTSIDE).await;
    assert!(matches!(outcome, Err(EntityError::NotFound { .. })), "{outcome:?}");
    assert_eq!(committed, 0, "nothing is committed");
}

/// Clause 5.6.6.4 on the path that reads the removed state for a notification.
#[tokio::test]
async fn a_delete_of_a_read_entity_outside_the_type_selector_is_not_found() {
    let (outcome, committed) = deleted_under(DeletionWatchers::Possible, OUTSIDE).await;
    assert!(matches!(outcome, Err(EntityError::NotFound { .. })), "{outcome:?}");
    assert_eq!(committed, 0, "nothing is committed");
}

/// Clause 4.17: an entity satisfying the selector is deleted on either path.
#[tokio::test]
async fn a_delete_inside_the_type_selector_proceeds() {
    for watchers in [DeletionWatchers::None, DeletionWatchers::Possible] {
        for raw in INSIDE {
            let (outcome, committed) = deleted_under(watchers, raw).await;
            assert!(outcome.is_ok(), "{raw}: {outcome:?}");
            assert_eq!(committed, 1, "{raw}: the deletion lands");
        }
    }
}
