use proc_macro2::Span;
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use syn::{
    LitStr,
    Result,
    Token,
    parse::{Parse, ParseStream},
};

/// Parsed macro input containing exactly three local JSON-LD context paths.
pub struct MacroInput {
    /// Path to the NGSI-LD core context.
    pub core: LitStr,
    /// Path to the Data Integrity companion context.
    pub data_integrity: LitStr,
    /// Path to the Andromeda-owned context.
    pub andromeda: LitStr,
}

impl Parse for MacroInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let core: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let data_integrity: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let andromeda: LitStr = input.parse()?;
        let _ = input.parse::<Option<Token![,]>>()?;

        if !input.is_empty() {
            return Err(input.error("expected exactly three string literal context paths"));
        }

        Ok(Self {
            core,
            data_integrity,
            andromeda,
        })
    }
}

/// A resolved input context file.
#[derive(Clone, Debug)]
pub struct InputContextFile {
    /// Logical name of the input file for diagnostics.
    pub label: &'static str,
    /// Resolved absolute path to the JSON-LD file.
    pub resolved_path: PathBuf,
    /// Literal span for diagnostics.
    pub span: Span,
    /// Canonical remote URLs that are allowed to resolve to this file.
    pub remote_urls: &'static [&'static str],
}

/// All three resolved input contexts.
#[derive(Clone, Debug)]
pub struct InputContexts {
    /// NGSI-LD core context file.
    pub core: InputContextFile,
    /// Data Integrity context file.
    pub data_integrity: InputContextFile,
    /// Andromeda-owned context file.
    pub andromeda: InputContextFile,
}

impl InputContexts {
    /// Resolve the macro input relative to the call-site base directory.
    pub fn resolve(base_dir: PathBuf, input: MacroInput) -> Result<InputContexts> {
        Ok(Self {
            core: resolve_input(
                &base_dir,
                "core",
                input.core,
                &["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"],
            )?,
            data_integrity: resolve_input(
                &base_dir,
                "data_integrity",
                input.data_integrity,
                &["https://w3id.org/security/data-integrity/v2"],
            )?,
            andromeda: resolve_input(&base_dir, "andromeda", input.andromeda, &[])?,
        })
    }

    /// Iterate over the three files in merge order.
    pub fn iter(&self) -> impl Iterator<Item = &InputContextFile> {
        [&self.core, &self.data_integrity, &self.andromeda].into_iter()
    }

    /// Find an input file by an allowed remote URL.
    pub fn find_by_remote_url(&self, url: &str) -> Option<&InputContextFile> {
        self.iter().find(|file| file.remote_urls.contains(&url))
    }
}

fn resolve_input(base_dir: &Path, label: &'static str, lit: LitStr, remote_urls: &'static [&'static str]) -> Result<InputContextFile> {
    let relative_path = lit.value();
    let resolved_path = base_dir.join(&relative_path);

    Ok(InputContextFile {
        label,
        resolved_path,
        span: lit.span(),
        remote_urls,
    })
}

/// A parsed JSON-LD context document.
#[derive(Clone, Debug)]
pub struct ParsedDocument {
    /// Metadata about the source file.
    pub input: InputContextFile,
    /// Parsed root JSON value.
    pub root: Value,
}

impl ParsedDocument {
    /// Return the top-level `@context` object.
    pub fn top_level_context(&self) -> syn::Result<&Map<String, Value>> {
        self.root.get("@context").and_then(Value::as_object).ok_or_else(|| {
            syn::Error::new(
                self.input.span,
                format!(
                    "{} context {} is missing a top-level @context object",
                    self.input.label,
                    self.input.resolved_path.display()
                ),
            )
        })
    }
}

/// Source information for a discovered prefix or term.
#[derive(Clone, Debug)]
pub struct DefinitionSource {
    /// Source file path.
    pub path: PathBuf,
}

/// Prefix definition extracted from the JSON-LD contexts.
#[derive(Clone, Debug)]
pub struct PrefixDefinition {
    /// Compact prefix key such as `ngsi-ld`.
    pub compact: String,
    /// Expanded namespace IRI.
    pub expanded: String,
    /// Definition origin.
    pub source: DefinitionSource,
}

/// How a term's `@id` was declared before resolution.
#[derive(Clone, Debug)]
pub enum TermTarget {
    /// Absolute IRI.
    Absolute(String),
    /// Prefixed name such as `ngsi-ld:Property`.
    Prefixed { prefix: String, suffix: String },
    /// Local compact term reference.
    LocalReference(String),
}

/// Term definition extracted from the JSON-LD contexts.
#[derive(Clone, Debug)]
pub struct TermDefinition {
    /// Compact term key.
    pub compact: String,
    /// Unresolved identity target.
    pub target: TermTarget,
    /// Definition origin.
    pub source: DefinitionSource,
}

/// Resolved prefix constant data.
#[derive(Clone, Debug)]
pub struct ResolvedPrefix {
    /// Compact prefix key.
    pub compact: String,
    /// Expanded namespace IRI.
    pub expanded: String,
    /// Rust constant name.
    pub const_name: String,
}

/// Resolved term constant data.
#[derive(Clone, Debug)]
pub struct ResolvedTerm {
    /// Compact term key.
    pub compact: String,
    /// Expanded term IRI.
    pub expanded: String,
    /// Rust constant name.
    pub const_name: String,
}

/// Final resolved vocabulary model.
#[derive(Clone, Debug)]
pub struct ResolvedModel {
    /// Resolved prefixes keyed by compact name.
    pub prefixes: Vec<ResolvedPrefix>,
    /// Resolved terms keyed by compact name.
    pub terms: Vec<ResolvedTerm>,
    /// Absolute paths to include for rebuild tracking.
    pub include_paths: Vec<String>,
}

/// Mutable model built while scanning all input contexts.
#[derive(Clone, Debug, Default)]
pub struct ContextModel {
    /// Prefixes keyed by compact prefix.
    pub prefixes: BTreeMap<String, PrefixDefinition>,
    /// Terms keyed by compact term.
    pub terms: BTreeMap<String, TermDefinition>,
}
