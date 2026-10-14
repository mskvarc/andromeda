//! The `@context` a registration's forwarded requests are compacted against,
//! dereferenced when the registration is written.
//!
//! Clause 4.3.6.6 has the broker compact every request it distributes to a
//! registered Context Source against the context the `jsonldContext` key of the
//! registration's `contextSourceInfo` names. One that cannot be retrieved or
//! processed would fail each of those requests long after the registration was
//! accepted, so it is resolved and processed when a registration is created
//! and when an update replaces its `contextSourceInfo`: an unreachable context
//! is `LdContextNotAvailable` and an invalid one `BadRequestData` at the write,
//! as clauses 5.8.1.4 and 5.8.2.4 answer a subscription's `jsonldContext`.

use crate::{RegistrationService, error::RegistrationError};
use andromeda_jsonld::context_validation::dereference_context_reference;
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfo;

impl RegistrationService {
    /// Resolve and process the `jsonldContext` the registration's
    /// `contextSourceInfo` names, if it names one.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::JsonLd`] carrying the context-loading
    /// failure when the context cannot be retrieved, and the JSON-LD
    /// processing failure when it is not a valid `@context`.
    pub(crate) async fn dereference_forwarding_context(&self, info: &ContextSourceInfo<'_>) -> Result<(), RegistrationError> {
        if let Some(reference) = &info.jsonld_context {
            dereference_context_reference(&self.engine, reference).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        RegistrationService,
        error::RegistrationError,
        fixed_store::{FixedStore, registration},
        registration_body::RegistrationBody,
        service_fixture::{offline_engine, service_over},
        store::RegistrationStore,
    };
    use andromeda_jsonld::{
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
        error::JsonLdError,
        user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, request_body::RequestBody};
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        env,
        fs,
        process,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };
    use url::Url;

    const REGISTRATION_ID: &str = "urn:ngsi-ld:ContextSourceRegistration:forwarding-context";
    const ABSENT: &str = "https://example.org/absent.jsonld";
    const VALID: &str = "https://example.org/valid.jsonld";
    const INVALID: &str = "https://example.org/invalid.jsonld";

    /// Makes every temporary context file name unique, so engines built by
    /// tests running in parallel never write or remove each other's files.
    static CONTEXT_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    /// An engine resolving `VALID` to a context and `INVALID` to a document
    /// whose term definition is a number, and nothing else.
    fn serving_engine() -> JsonLdEngine {
        let documents = [
            (VALID, r#"{"@context": {"speed": "https://example.org/speed"}}"#),
            (INVALID, r#"{"@context": {"speed": 42}}"#),
        ];
        let sequence = CONTEXT_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut preloads = HashMap::new();
        for (index, (url, document)) in documents.iter().enumerate() {
            let path = env::temp_dir().join(format!("andromeda-registration-forwarding-context-{}-{sequence}-{index}.jsonld", process::id()));
            fs::write(&path, document).expect("the temporary directory is writable");
            preloads.insert(Url::parse(url).expect("a valid URL"), path);
        }
        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads.clone()) // clone: the removal below walks the paths the builder takes
            .build()
            .expect("engine builds");
        for path in preloads.values() {
            fs::remove_file(path).expect("the temporary context is removable");
        }
        engine
    }

    fn service(store: &Arc<FixedStore>, engine: JsonLdEngine) -> RegistrationService {
        service_over(Arc::clone(store) as Arc<dyn RegistrationStore>, engine, None) // clone: the test keeps its own handle to read what was written
    }

    async fn core_context(service: &RegistrationService) -> AdmittedContext {
        admit_user_context(service.engine(), SubmittedContext::from(ContextSpec::none()))
            .await
            .expect("the core context is admitted")
    }

    fn forwarding_info(context: &str) -> Value {
        json!([{"key": "jsonldContext", "value": context}])
    }

    fn registration_body(context: &str) -> RegistrationBody {
        let document = json!({
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "contextSourceInfo": forwarding_info(context)
        });
        RegistrationBody::from_request_text(document.to_string().as_bytes()).expect("a registration body")
    }

    fn fragment(document: &Value) -> RegistrationBody {
        RegistrationBody::from_request_text(document.to_string().as_bytes()).expect("a registration fragment")
    }

    fn registration_id() -> Identifier {
        REGISTRATION_ID.parse().expect("valid URI")
    }

    /// The stored registration, already forwarding under `context`.
    fn stored_forwarding_under(context: &str) -> Arc<FixedStore> {
        let mut stored = registration(REGISTRATION_ID, "inclusive", &json!([{"entities": [{"type": "https://example.org/Vehicle"}]}]));
        stored.access.context_source_info = Some(vec![KeyValuePair {
            key: "jsonldContext".into(),
            value: context.into(),
        }]);
        Arc::new(FixedStore::new(vec![stored]))
    }

    fn json_ld_failure(error: RegistrationError) -> JsonLdError {
        let RegistrationError::JsonLd(failure) = error else {
            panic!("expected a JSON-LD failure, got {error:?}");
        };
        failure
    }

    #[tokio::test]
    async fn a_registration_forwarding_under_a_context_that_loads_and_processes_is_created() {
        let service = service(&Arc::new(FixedStore::new(Vec::new())), serving_engine());
        let context = core_context(&service).await;
        service.create(registration_body(VALID), &context).await.expect("the registration is created");
    }

    /// Clause 5.5.6: a remote context that cannot be retrieved is
    /// `LdContextNotAvailable`.
    #[tokio::test]
    async fn a_registration_forwarding_under_an_unreachable_context_is_refused_as_unavailable() {
        let service = service(&Arc::new(FixedStore::new(Vec::new())), offline_engine());
        let context = core_context(&service).await;
        let error = service
            .create(registration_body(ABSENT), &context)
            .await
            .expect_err("the context cannot be retrieved");
        let failure = json_ld_failure(error);
        assert!(failure.is_context_not_available(), "{failure}");
    }

    /// Clause 5.5.6: a remote context retrieved but invalid is `BadRequestData`,
    /// not `LdContextNotAvailable`.
    #[tokio::test]
    async fn a_registration_forwarding_under_an_invalid_context_is_refused_as_invalid() {
        let service = service(&Arc::new(FixedStore::new(Vec::new())), serving_engine());
        let context = core_context(&service).await;
        let error = service.create(registration_body(INVALID), &context).await.expect_err("the context is invalid");
        let failure = json_ld_failure(error);
        assert!(!failure.is_context_not_available(), "{failure}");
    }

    #[tokio::test]
    async fn an_update_replacing_the_context_source_info_with_an_unreachable_context_is_refused_and_writes_nothing() {
        let store = stored_forwarding_under(VALID);
        let service = service(&store, offline_engine());
        let context = core_context(&service).await;
        let error = service
            .update(&registration_id(), fragment(&json!({"contextSourceInfo": forwarding_info(ABSENT)})), &context)
            .await
            .expect_err("the context cannot be retrieved");
        let failure = json_ld_failure(error);
        assert!(failure.is_context_not_available(), "{failure}");
        assert!(store.updated_information().is_empty());
    }

    /// A stored forwarding context was dereferenced when it was written, so an
    /// update leaving `contextSourceInfo` alone is not refused because that
    /// context went away since.
    #[tokio::test]
    async fn an_update_leaving_the_context_source_info_alone_does_not_dereference_it() {
        let store = stored_forwarding_under(ABSENT);
        let service = service(&store, offline_engine());
        let context = core_context(&service).await;
        service
            .update(&registration_id(), fragment(&json!({"registrationName": "renamed"})), &context)
            .await
            .expect("the update applies");
        pretty_assertions::assert_eq!(store.updated_information().len(), 1);
    }
}
