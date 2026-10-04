//! Typed expansion against the processor's, over the request bodies the
//! integration suites write (design section 11.2, clause 5.5.7).
//!
//! `tests/fixtures/bodies/suite-bodies.json` holds every plain-JSON write body
//! of `tests/**` and the annex C corpus entities, each with the file it came
//! from. Every body is expanded every way a write may carry it -- as a typed
//! entity, as a typed merge fragment, as a JSON fragment under both concise
//! rewrites, and an attribute instance as the fragment of clauses 5.6.4 and
//! 5.6.19 -- and wherever the walk expands it, the processor must read it as
//! the same document, or refuse it with the same refusal. The bodies the walk
//! declines are counted and named.

use crate::{
    expansion_fixture::{BothWays, FixtureFailure, TERMS, VOCAB_AND_LANGUAGE, attribute, decoded_both_ways, engine, entity, fragment, patch},
    normalization::Normalization,
    typed_expansion::TypedExpansion,
};
use serde::Deserialize;
use serde_json::Value;
use std::{fmt::Debug, fs, path::PathBuf};

/// One body of the corpus.
#[derive(Deserialize)]
struct CorpusBody {
    /// The suite file the body was written in.
    source: String,
    /// What the body is: `entity`, `fragment` or `attribute`.
    kind: String,
    /// The body.
    body: Value,
}

/// What the comparison found over the corpus.
#[derive(Default)]
struct Tally {
    /// Expansions both ways read alike.
    agreed: usize,
    /// Expansions both ways refused alike.
    refused: usize,
    /// Expansions the walk declined, by body and reason.
    declined: Vec<String>,
    /// Bodies a way could not take (not typed, context unavailable offline).
    skipped: Vec<String>,
    /// Expansions the two ways read differently.
    disagreed: Vec<String>,
}

impl Tally {
    /// Record one comparison.
    fn record<T: Debug + PartialEq>(&mut self, label: &str, both: Result<BothWays<T>, FixtureFailure>) {
        let both = match both {
            Ok(both) => both,
            Err(failure) => {
                self.skipped.push(format!("{label}: {}", failure.reason()));
                return;
            }
        };
        match (both.typed, both.processor) {
            (Ok(TypedExpansion::Expanded(typed)), Ok(processor)) if typed == processor => self.agreed += 1,
            (Ok(TypedExpansion::Declined(decline)), _) => self.declined.push(format!("{label}: {decline}")),
            (Err(typed), Err(processor)) if typed.to_string() == processor.to_string() => self.refused += 1,
            (typed, processor) => self.disagreed.push(format!("{label}:\n  typed     {typed:?}\n  processor {processor:?}")),
        }
    }
}

/// The bodies of the corpus.
fn corpus() -> Vec<CorpusBody> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bodies/suite-bodies.json");
    serde_json::from_str(&fs::read_to_string(path).expect("the corpus is readable")).expect("the corpus parses")
}

/// `body` written under the test context `context` instead of its own, when
/// it names none of its own.
fn under(context: &str, body: &Value) -> Option<Value> {
    let mut members = body.as_object()?.clone(); // clone: the corpus body is expanded under every context
    if members.contains_key("@context") {
        return None;
    }
    members.insert("@context".to_owned(), Value::String(context.to_owned()));
    Some(Value::Object(members))
}

#[tokio::test]
async fn typed_expansion_equals_processor_expansion_over_the_suite_bodies() {
    let engine = engine();
    let mut tally = Tally::default();
    for (index, item) in corpus().into_iter().enumerate() {
        let label = format!("#{index} {} ({})", item.source, item.kind);
        if item.kind == "attribute" {
            tally.record(&format!("{label} attribute"), attribute(&engine, "attribute", item.body).await);
            continue;
        }
        if item.kind == "entity" {
            tally.record(&format!("{label} entity"), entity(&engine, item.body.clone(), Normalization::Entity).await); // clone: the body is expanded every way
        }
        tally.record(
            &format!("{label} merge"),
            patch(&engine, item.body.clone(), Normalization::MemberDeletions).await,
        ); // clone: the body is expanded every way
        tally.record(&format!("{label} update"), fragment(&engine, item.body.clone(), Normalization::Entity).await); // clone: the body is expanded every way
        for context in [TERMS, VOCAB_AND_LANGUAGE] {
            if let Some(body) = under(context, &item.body) {
                tally.record(&format!("{label} update under {context}"), fragment(&engine, body, Normalization::Entity).await);
            }
        }
        tally.record(
            &format!("{label} merge-json"),
            fragment(&engine, item.body, Normalization::MemberDeletions).await,
        );
    }
    println!(
        "agreed {}, refused alike {}, declined {}, skipped {}",
        tally.agreed,
        tally.refused,
        tally.declined.len(),
        tally.skipped.len()
    );
    for declined in &tally.declined {
        println!("declined {declined}");
    }
    for skipped in &tally.skipped {
        println!("skipped {skipped}");
    }
    assert!(
        tally.disagreed.is_empty(),
        "the typed walk and the processor disagree:\n{}",
        tally.disagreed.join("\n")
    );
    assert!(tally.agreed > 600, "the corpus exercised too little: {} agreed", tally.agreed);
}

/// The processor decoding reads the processor's typed output as the canonical
/// decoder of the entity crate reads the same document as expanded text:
/// wherever both accept a document they read the same written document, and
/// both refuse the same documents.
///
/// Two deliberate differences are named rather than hidden. The entity's own
/// `createdAt`, `modifiedAt` and `deletedAt` are the broker's (clause 4.8): a
/// written document that carries them carries nothing the entity keeps, so
/// the decoding drops them where the canonical decoder of stored state refuses
/// them. And a concise `objectList` (clause 4.5.22.3) expands to value objects
/// under the core context, which the decoding reads as bare targets and the
/// canonical decoder does not read yet.
#[tokio::test]
async fn processor_decoding_reads_as_the_canonical_decoder_reads() {
    let engine = engine();
    let mut disagreed = Vec::new();
    let mut divergent = Vec::new();
    let mut agreed = 0_usize;
    for (index, item) in corpus().into_iter().enumerate() {
        if item.kind == "attribute" {
            continue;
        }
        let Ok((decoded, canonical)) = decoded_both_ways(&engine, item.body, Normalization::Entity).await else {
            continue;
        };
        match (decoded, canonical) {
            (Ok(decoded), Ok(canonical)) if decoded == canonical => agreed += 1,
            (Err(_), Err(_)) => agreed += 1,
            (Ok(decoded), Ok(canonical)) => disagreed.push(format!("#{index} {}:\n  decoded   {decoded:?}\n  canonical {canonical:?}", item.source)),
            (decoded, canonical) => divergent.push(format!(
                "#{index} {}: decoded {}, canonical {}",
                item.source,
                decoded.map_or_else(|refusal| refusal.to_string(), |_| "accepted".to_owned()),
                canonical.map_or_else(|refusal| refusal.to_string(), |_| "accepted".to_owned())
            )),
        }
    }
    println!("agreed {agreed}, disagreed {}, divergent {}", disagreed.len(), divergent.len());
    for line in &divergent {
        println!("divergent {line}");
    }
    assert!(
        disagreed.is_empty(),
        "the processor decoding and the canonical decoder read one document differently:\n{}",
        disagreed.join("\n")
    );
    assert!(
        divergent.len() <= 3,
        "more documents than the named differences account for:\n{}",
        divergent.join("\n")
    );
}
