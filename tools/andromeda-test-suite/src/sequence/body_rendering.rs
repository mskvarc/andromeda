//! The forms the `ngsi-ld` crate's typed model of a document writes it in, and why a model refuses a document.

use crate::sequence::body::Document;
use ngsi_ld::{
    model::{
        Attribute,
        Change,
        Complete,
        DocumentError,
        Entity,
        EntityFragment,
        Input,
        InstanceDeletion,
        LocalContext,
        Lossless,
        Patch,
        Projected,
        Representation,
        evolution::TemporalEntity,
    },
    registration::{CSourceRegistration, Problem as RegistrationProblem},
    snapshot::{EntityQuery, Problem as SnapshotProblem, QueryDocument, Snapshot, TemporalEvolutionQuery},
    subscription::{EntitySubscription, Problem as SubscriptionProblem, ReadErrorKind, RegistrationSubscription, Subscription},
};

/// Why the typed model of a document refuses it.
#[derive(Debug, thiserror::Error)]
pub enum ModelRefusal {
    /// The model refuses the document in both lossless forms.
    #[error("normalized: {normalized} | concise: {concise}")]
    Lossless {
        /// Why the normalized form is refused.
        normalized: Box<DocumentError>,
        /// Why the concise form is refused.
        concise: Box<DocumentError>,
    },
    /// The subscription model refuses the document.
    #[error(transparent)]
    Subscription(Box<DocumentError<ReadErrorKind<SubscriptionProblem>>>),
    /// The registration subscription model refuses the document.
    #[error(transparent)]
    RegistrationSubscription(Box<DocumentError<ReadErrorKind<SubscriptionProblem>>>),
    /// The registration model refuses the document.
    #[error(transparent)]
    Registration(Box<DocumentError<ReadErrorKind<RegistrationProblem>>>),
    /// The snapshot model refuses the document.
    #[error(transparent)]
    Snapshot(Box<DocumentError<ReadErrorKind<SnapshotProblem>>>),
    /// The entity query model refuses the document.
    #[error(transparent)]
    EntityQuery(Box<DocumentError<ReadErrorKind<SnapshotProblem>>>),
    /// The temporal evolution query model refuses the document.
    #[error(transparent)]
    TemporalQuery(Box<DocumentError<ReadErrorKind<SnapshotProblem>>>),
    /// The local `@context` model refuses the document.
    #[error(transparent)]
    Context(Box<DocumentError>),
    /// An array of entity identifiers has no model.
    #[error("an array of identifiers has no model")]
    NoModel,
}

/// Reads `text` in the normalized form, then in the concise form.
fn forms(parse: impl Fn(Lossless) -> Result<Vec<String>, DocumentError>) -> Result<Vec<String>, ModelRefusal> {
    parse(Lossless::Normalized).or_else(|normalized| {
        parse(Lossless::Concise).map_err(|concise| ModelRefusal::Lossless {
            normalized: Box::new(normalized),
            concise: Box::new(concise),
        })
    })
}

/// Every form the model of `document` writes the document `text` in.
pub(crate) fn renderings(document: Document, text: &str) -> Result<Vec<String>, ModelRefusal> {
    match document {
        Document::Entity | Document::Entities => forms(|form| {
            Entity::<Input>::parse(text, form).map(|entity| {
                vec![
                    entity.render(Representation::Normalized).to_string(),
                    entity.render(Representation::Concise).to_string(),
                ]
            })
        }),
        Document::Fragment => forms(|form| {
            EntityFragment::<Projected>::parse(text, form).map(|fragment| {
                vec![
                    fragment.render(Lossless::Normalized).to_string(),
                    fragment.render(Lossless::Concise).to_string(),
                ]
            })
        }),
        Document::CompleteFragments => forms(|form| {
            EntityFragment::<Complete>::parse(text, form).map(|fragment| {
                vec![
                    fragment.render(Lossless::Normalized).to_string(),
                    fragment.render(Lossless::Concise).to_string(),
                ]
            })
        }),
        Document::AttributePatch => forms(|form| {
            Change::<Attribute<Patch>, InstanceDeletion>::parse(text, form)
                .map(|change| vec![change.render(Lossless::Normalized).to_string(), change.render(Lossless::Concise).to_string()])
        }),
        Document::AttributeInput => forms(|form| {
            Attribute::<Input>::parse_input(text, form).map(|attribute| {
                vec![
                    attribute.render(Lossless::Normalized).to_string(),
                    attribute.render(Lossless::Concise).to_string(),
                ]
            })
        }),
        Document::Temporal | Document::TemporalAttributes => forms(|form| {
            TemporalEntity::<Input>::parse(text, form).map(|temporal| {
                vec![
                    temporal.render(Lossless::Normalized).to_string(),
                    temporal.render(Lossless::Concise).to_string(),
                ]
            })
        }),
        Document::Subscription => Subscription::<Input, EntitySubscription>::parse(text)
            .map(|subscription| vec![subscription.render().to_string()])
            .map_err(|error| ModelRefusal::Subscription(Box::new(error))),
        Document::RegistrationSubscription => Subscription::<Input, RegistrationSubscription>::parse(text)
            .map(|subscription| vec![subscription.render().to_string()])
            .map_err(|error| ModelRefusal::RegistrationSubscription(Box::new(error))),
        Document::Registration => CSourceRegistration::<Input>::parse(text)
            .map(|registration| vec![registration.render().to_string()])
            .map_err(|error| ModelRefusal::Registration(Box::new(error))),
        Document::Snapshot => Snapshot::<Input>::parse(text)
            .map(|snapshot| vec![snapshot.render().to_string()])
            .map_err(|error| ModelRefusal::Snapshot(Box::new(error))),
        Document::EntityQuery => QueryDocument::<EntityQuery>::parse(text)
            .map(|query| vec![query.render().to_string()])
            .map_err(|error| ModelRefusal::EntityQuery(Box::new(error))),
        Document::TemporalQuery => QueryDocument::<TemporalEvolutionQuery>::parse(text)
            .map(|query| vec![query.render().to_string()])
            .map_err(|error| ModelRefusal::TemporalQuery(Box::new(error))),
        Document::Context => LocalContext::parse(text)
            .map(|context| vec![context.render().to_string()])
            .map_err(|error| ModelRefusal::Context(Box::new(error))),
        Document::EntityIds => Err(ModelRefusal::NoModel),
    }
}
