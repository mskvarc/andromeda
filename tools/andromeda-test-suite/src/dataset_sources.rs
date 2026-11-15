//! Context sources registered for different `datasetId`s, and the ones a broker forwards a request to.
//!
//! Clause 5.12 matches a request naming a `datasetId` with a registration that lists one of its values or lists no
//! `datasetId` at all. A test registers one source per [`DatasetBinding`], all served by one mock server and told apart
//! by the prefix their endpoints name, sends its request, and reads which prefixes received it.

use crate::{broker::BrokerClient, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, source_request::SourceRequest};
use googletest::prelude::*;
use ngsi_ld::{
    model::DatasetId,
    registration::{CSourceRegistration, DistributedOperation, RegistrationInfo, RegistrationMode},
};
use wiremock::ResponseTemplate;

/// One source: the prefix its endpoint names, which also suffixes its registration identifier as
/// `Namespace::registration_id` writes it, and the `datasetId`s the registration lists, none when absent.
#[derive(Debug, Clone)]
pub struct DatasetBinding {
    /// The prefix of the endpoint, and the suffix of the registration identifier.
    pub prefix: &'static str,
    /// The `datasetId` member of the registration (Table 5.2.9-1).
    pub dataset_ids: Option<Vec<DatasetId>>,
}

/// The sources one test registered, whose registrations are deleted when the value is dropped.
#[derive(Debug)]
pub struct DatasetBoundSources {
    /// The mock server every source answers from.
    source: ContextSource,
    /// The prefixes of the sources, in registration order.
    prefixes: Vec<&'static str>,
    /// The removal of the registrations.
    _cleanup: Cleanup,
}

impl DatasetBoundSources {
    /// Registers with `broker` one inclusive source for each of `bindings`, covering `information` and advertising
    /// `operations`.
    ///
    /// # Errors
    ///
    /// Returns the failure of starting the mock server, of building a registration or of registering it.
    pub async fn register(
        broker: &BrokerClient,
        namespace: &Namespace,
        information: &RegistrationInfo,
        operations: &[DistributedOperation],
        bindings: Vec<DatasetBinding>,
    ) -> Result<DatasetBoundSources> {
        let source = ContextSource::start().await?;
        // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
        // cooldown that opens there holds back a later test's registration naming the same endpoint.
        let mut cleanup = Cleanup::of(broker);
        let mut prefixes = Vec::with_capacity(bindings.len());
        for binding in bindings {
            let registration_id = namespace.registration_id(binding.prefix)?;
            let registration = CSourceRegistration::builder(source.endpoint(binding.prefix)?)
                .id(registration_id.clone())
                .information([information.clone()])
                .mode(RegistrationMode::Inclusive)
                .operations(operations.iter().copied());
            let registration = match binding.dataset_ids {
                Some(dataset_ids) => registration.dataset_ids(dataset_ids).build()?,
                None => registration.build()?,
            };
            broker.registrations().register(&registration).send().await?;
            cleanup = cleanup.registration(&registration_id);
            prefixes.push(binding.prefix);
        }
        Ok(DatasetBoundSources {
            source,
            prefixes,
            _cleanup: cleanup,
        })
    }

    /// Has every source answer the requests `request` describes for its prefix with what `answer` writes for it.
    ///
    /// # Errors
    ///
    /// Returns the failure of mounting an answer.
    pub async fn answer(&self, request: impl Fn(&str) -> SourceRequest, answer: impl Fn(&str) -> ResponseTemplate) -> Result<()> {
        for prefix in &self.prefixes {
            self.source.answer_when(request(prefix), answer(prefix)).await?;
        }
        Ok(())
    }

    /// The prefixes, in registration order, whose source received a request `request` describes for that prefix.
    ///
    /// # Errors
    ///
    /// Returns the failure of reading what a source received.
    pub async fn reached(&self, request: impl Fn(&str) -> SourceRequest) -> Result<Vec<&'static str>> {
        let mut reached = Vec::new();
        for prefix in &self.prefixes {
            if !self.source.received_matching(&request(prefix)).await?.is_empty() {
                reached.push(*prefix);
            }
        }
        Ok(reached)
    }
}
