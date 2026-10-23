//! What a client's request makes of every attribute member shape: the body is
//! decoded under the aliases of its admitted context and admitted as the write
//! names, a creation (clause 5.6.1) and a merge (clause 5.6.17) alike.

use crate::{
    admission_fixture::admitted_context,
    notification_recording_harness::{ENTITY_ID, recording_service},
    write_kind::{CreateEntity, MergeEntity},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::{
    attribute::merge_member::MergeMember,
    entity::{Entity, EntityPatch},
    request_body::RequestBody,
};
use andromeda_test_fixtures::attribute_member_shape::{ShapeCase, Verdict};

/// The context a case's member is written under.
fn context_of(case: &ShapeCase) -> ContextSpec {
    case.context.map_or_else(ContextSpec::core_only, |terms| {
        ContextSpec::from_body(JsonLiteral::admit(terms).expect("the case context is JSON"))
    })
}

/// The written payload of a creation of the Building carrying the case's
/// member, decoded and admitted, or the refusal of it.
async fn created(case: &ShapeCase) -> Result<String, String> {
    let (service, _recorded) = recording_service().await;
    let context = admitted_context(&service, &context_of(case)).await;
    let text = format!(r#"{{"id": "{ENTITY_ID}", "type": "Building", "attr": {}}}"#, case.member);
    let entity = <Entity>::from_request_text(text.as_bytes(), context.aliases()).map_err(|error| error.to_string())?;
    service
        .admit_entity_write::<CreateEntity>(&entity, &context)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&entity).map_err(|error| error.to_string())
}

/// The written payload of a merge of the fragment carrying the case's member,
/// decoded and admitted, or the refusal of it.
async fn merged(case: &ShapeCase) -> Result<String, String> {
    let (service, _recorded) = recording_service().await;
    let context = admitted_context(&service, &context_of(case)).await;
    let text = format!(r#"{{"attr": {}}}"#, case.member);
    let patch = <EntityPatch<MergeMember>>::from_request_text(text.as_bytes(), context.aliases()).map_err(|error| error.to_string())?;
    service
        .admit_entity_write::<MergeEntity>(&patch, &context)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::to_string(&patch).map_err(|error| error.to_string())
}

/// Assert that `outcome`, the reading of `case` by `route`, is `verdict`.
fn assert_outcome(case: &ShapeCase, route: &str, verdict: Verdict, outcome: &Result<String, String>) {
    match verdict {
        Verdict::Typed => assert!(outcome.is_ok(), "{} by {route}: {outcome:?}", case.name),
        Verdict::Refused => assert!(outcome.is_err(), "{} by {route}: {outcome:?}", case.name),
        Verdict::Ignored(member) => match outcome {
            Ok(written) => assert!(!written.contains(member), "{} by {route} keeps {member}: {written}", case.name),
            Err(refusal) => panic!("{} by {route}: {refusal}", case.name),
        },
        Verdict::NotApplicable => {}
    }
}

/// Assert what a creation and a merge make of `case`.
async fn assert_verdict(case: &ShapeCase) {
    assert_outcome(case, "a creation", case.request, &created(case).await);
    assert_outcome(case, "a merge", case.merge, &merged(case).await);
}

/// One test per shape, named after it.
macro_rules! shape_tests {
    ($($test:ident => $case:path),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $test() {
                assert_verdict(&$case).await;
            }
        )+
    };
}

andromeda_test_fixtures::for_each_attribute_member_shape!(shape_tests);
