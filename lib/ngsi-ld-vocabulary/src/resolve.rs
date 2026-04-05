use crate::model::{
    ContextModel,
    DefinitionSource,
    InputContextFile,
    InputContexts,
    ParsedDocument,
    PrefixDefinition,
    ResolvedModel,
    ResolvedPrefix,
    ResolvedTerm,
    TermDefinition,
    TermTarget,
};
use heck::ToShoutySnakeCase;
use proc_macro::Span as ProcMacroSpan;
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};
use syn::LitStr;

/// Resolve the base directory used for call-site-relative macro paths.
pub fn resolve_call_site_base_dir(path_literal: &LitStr) -> syn::Result<PathBuf> {
    if let Some(source_file_path) = path_literal.span().unwrap().source().local_file()
        && let Some(parent) = source_file_path.parent()
    {
        let parent = parent.to_path_buf();
        return Ok(std::fs::canonicalize(&parent).unwrap_or(parent));
    }

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|error| {
        syn::Error::new(
            path_literal.span(),
            format!("unable to resolve call-site directory and CARGO_MANIFEST_DIR is not set: {error}"),
        )
    })?;
    let manifest_dir = PathBuf::from(manifest_dir);

    Ok(std::fs::canonicalize(&manifest_dir).unwrap_or(manifest_dir))
}

/// Read, merge, and resolve the three input contexts into a canonical vocabulary model.
pub fn load_contexts(inputs: &InputContexts) -> syn::Result<ResolvedModel> {
    let documents = inputs.iter().cloned().map(read_document).collect::<syn::Result<Vec<_>>>()?;

    let mut model = ContextModel::default();
    for document in &documents {
        collect_context_object(document.top_level_context()?, &document.input, inputs, &mut model)?;
    }

    let prefixes = resolve_prefixes(model.prefixes)?;
    let terms = resolve_terms(model.terms, &prefixes)?;
    let include_paths = documents
        .iter()
        .map(|document| path_to_string(&document.input.resolved_path, document.input.span))
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(ResolvedModel {
        prefixes,
        terms,
        include_paths,
    })
}

fn read_document(input: InputContextFile) -> syn::Result<ParsedDocument> {
    let input = InputContextFile {
        resolved_path: std::fs::canonicalize(&input.resolved_path).unwrap_or(input.resolved_path.clone()),
        ..input
    };
    let json = std::fs::read_to_string(&input.resolved_path).map_err(|error| {
        syn::Error::new(
            input.span,
            format!("cannot read {} context file {}: {error}", input.label, input.resolved_path.display()),
        )
    })?;
    let root = serde_json::from_str(&json).map_err(|error| {
        syn::Error::new(
            input.span,
            format!("invalid JSON in {} context file {}: {error}", input.label, input.resolved_path.display()),
        )
    })?;

    Ok(ParsedDocument { input, root })
}

fn collect_context_object(context: &Map<String, Value>, file: &InputContextFile, inputs: &InputContexts, model: &mut ContextModel) -> syn::Result<()> {
    for (compact, value) in context {
        if compact.starts_with('@') {
            continue;
        }

        if let Some(prefix_iri) = value.as_str().filter(|candidate| is_prefix_namespace(candidate)) {
            insert_prefix(model, compact, prefix_iri, file)?;
            continue;
        }

        if let Some(target) = extract_term_target(compact, value)? {
            insert_term(model, compact, target, file)?;
        }

        collect_nested_contexts(value, file, inputs, model)?;
    }

    Ok(())
}

fn collect_nested_contexts(value: &Value, file: &InputContextFile, inputs: &InputContexts, model: &mut ContextModel) -> syn::Result<()> {
    match value {
        Value::Object(object) => {
            if let Some(nested) = object.get("@context") {
                collect_context_value(nested, file, inputs, model)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_nested_contexts(item, file, inputs, model)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }

    Ok(())
}

fn collect_context_value(value: &Value, file: &InputContextFile, inputs: &InputContexts, model: &mut ContextModel) -> syn::Result<()> {
    match value {
        Value::Object(object) => collect_context_object(object, file, inputs, model),
        Value::Array(items) => {
            for item in items {
                collect_context_value(item, file, inputs, model)?;
            }
            Ok(())
        }
        Value::String(url) => {
            if inputs.find_by_remote_url(url).is_some() {
                return Ok(());
            }

            Err(syn::Error::new(
                file.span,
                format!(
                    "unsupported remote @context URL {url} in {}; allowed URLs are {}",
                    file.resolved_path.display(),
                    inputs.iter().flat_map(|input| input.remote_urls.iter().copied()).collect::<Vec<_>>().join(", ")
                ),
            ))
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => Err(syn::Error::new(
            file.span,
            format!(
                "unsupported nested @context value in {}: expected object, array, or string URL",
                file.resolved_path.display()
            ),
        )),
    }
}

fn extract_term_target(compact: &str, value: &Value) -> syn::Result<Option<TermTarget>> {
    match value {
        Value::String(raw) => parse_term_target(compact, raw),
        Value::Object(object) => {
            let Some(id) = object.get("@id").and_then(Value::as_str) else {
                return Ok(None);
            };
            parse_term_target(compact, id)
        }
        Value::Array(_) | Value::Null | Value::Bool(_) | Value::Number(_) => Ok(None),
    }
}

fn parse_term_target(compact: &str, raw: &str) -> syn::Result<Option<TermTarget>> {
    if compact == "id" && raw == "@id" {
        return Ok(None);
    }
    if compact == "type" && raw == "@type" {
        return Ok(None);
    }
    if raw.starts_with('@') {
        return Ok(None);
    }
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("urn:") {
        return Ok(Some(TermTarget::Absolute(raw.to_string())));
    }
    if let Some((prefix, suffix)) = split_prefixed(raw) {
        return Ok(Some(TermTarget::Prefixed {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
        }));
    }
    if let Ok(iri) = iref::IriBuf::new(raw.to_string()) {
        return Ok(Some(TermTarget::Absolute(iri.to_string())));
    }
    if raw.is_empty() {
        return Err(syn::Error::new(
            ProcMacroSpan::call_site().into(),
            format!("term {compact} has an empty @id value"),
        ));
    }

    Ok(Some(TermTarget::LocalReference(raw.to_string())))
}

fn insert_prefix(model: &mut ContextModel, compact: &str, expanded: &str, file: &InputContextFile) -> syn::Result<()> {
    let definition = PrefixDefinition {
        compact: compact.to_string(),
        expanded: expanded.to_string(),
        source: source(file),
    };

    match model.prefixes.get(compact) {
        Some(previous) if previous.expanded == definition.expanded => Ok(()),
        Some(previous) => Err(syn::Error::new(
            file.span,
            format!(
                "prefix {compact} in {} conflicts with {} from {}",
                file.resolved_path.display(),
                previous.expanded,
                previous.source.path.display()
            ),
        )),
        None => {
            model.prefixes.insert(compact.to_string(), definition);
            Ok(())
        }
    }
}

fn insert_term(model: &mut ContextModel, compact: &str, target: TermTarget, file: &InputContextFile) -> syn::Result<()> {
    let definition = TermDefinition {
        compact: compact.to_string(),
        target,
        source: source(file),
    };

    match model.terms.get(compact) {
        Some(previous) if term_targets_equal(&previous.target, &definition.target) => Ok(()),
        Some(previous) => Err(syn::Error::new(
            file.span,
            format!(
                "term {compact} in {} conflicts with previous definition from {}",
                file.resolved_path.display(),
                previous.source.path.display()
            ),
        )),
        None => {
            model.terms.insert(compact.to_string(), definition);
            Ok(())
        }
    }
}

fn resolve_prefixes(prefixes: BTreeMap<String, PrefixDefinition>) -> syn::Result<Vec<ResolvedPrefix>> {
    let mut resolved = Vec::with_capacity(prefixes.len());
    let mut names = HashMap::<String, String>::new();

    for (_, prefix) in prefixes {
        let const_name = prefix.compact.to_shouty_snake_case();
        if let Some(previous) = names.insert(const_name.clone(), prefix.compact.clone()) {
            return Err(syn::Error::new(
                ProcMacroSpan::call_site().into(),
                format!("prefix naming collision: {previous} and {} both map to {const_name}", prefix.compact),
            ));
        }
        resolved.push(ResolvedPrefix {
            compact: prefix.compact,
            expanded: prefix.expanded,
            const_name,
        });
    }

    Ok(resolved)
}

fn resolve_terms(terms: BTreeMap<String, TermDefinition>, prefixes: &[ResolvedPrefix]) -> syn::Result<Vec<ResolvedTerm>> {
    let prefix_map = prefixes
        .iter()
        .map(|prefix| (prefix.compact.as_str(), prefix.expanded.as_str()))
        .collect::<HashMap<_, _>>();

    let mut cache = HashMap::<String, String>::new();
    let mut resolved = Vec::with_capacity(terms.len());
    let term_names = assign_term_const_names(terms.keys())?;

    for compact in terms.keys() {
        let expanded = resolve_term(compact, &terms, &prefix_map, &mut cache, &mut Vec::new(), &mut HashSet::new())?;
        resolved.push(ResolvedTerm {
            compact: compact.clone(),
            expanded,
            const_name: term_names
                .get(compact)
                .cloned()
                .ok_or_else(|| syn::Error::new(ProcMacroSpan::call_site().into(), format!("missing generated constant name for {compact}")))?,
        });
    }

    Ok(resolved)
}

fn resolve_term(
    compact: &str,
    terms: &BTreeMap<String, TermDefinition>,
    prefixes: &HashMap<&str, &str>,
    cache: &mut HashMap<String, String>,
    stack: &mut Vec<String>,
    visiting: &mut HashSet<String>,
) -> syn::Result<String> {
    if let Some(expanded) = cache.get(compact) {
        return Ok(expanded.clone());
    }
    if !visiting.insert(compact.to_string()) {
        stack.push(compact.to_string());
        return Err(syn::Error::new(
            ProcMacroSpan::call_site().into(),
            format!("cycle detected while resolving local term references: {}", stack.join(" -> ")),
        ));
    }
    stack.push(compact.to_string());

    let definition = terms
        .get(compact)
        .ok_or_else(|| syn::Error::new(ProcMacroSpan::call_site().into(), format!("unresolved local term reference: {compact}")))?;

    let expanded = match &definition.target {
        TermTarget::Absolute(iri) => iri.clone(),
        TermTarget::Prefixed { prefix, suffix } => {
            let namespace = prefixes.get(prefix.as_str()).ok_or_else(|| {
                syn::Error::new(
                    ProcMacroSpan::call_site().into(),
                    format!(
                        "unknown prefix {prefix} while resolving term {} from {}",
                        definition.compact,
                        definition.source.path.display()
                    ),
                )
            })?;
            format!("{namespace}{suffix}")
        }
        TermTarget::LocalReference(target) => resolve_term(target, terms, prefixes, cache, stack, visiting).map_err(|_| {
            syn::Error::new(
                ProcMacroSpan::call_site().into(),
                format!(
                    "unable to resolve local term reference {target} while resolving {} from {}",
                    definition.compact,
                    definition.source.path.display()
                ),
            )
        })?,
    };

    stack.pop();
    visiting.remove(compact);
    cache.insert(compact.to_string(), expanded.clone());
    Ok(expanded)
}

fn split_prefixed(value: &str) -> Option<(&str, &str)> {
    let (prefix, suffix) = value.split_once(':')?;
    if prefix.is_empty() || suffix.is_empty() {
        return None;
    }
    if !prefix.chars().all(is_prefix_char) {
        return None;
    }
    Some((prefix, suffix))
}

fn is_prefix_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
}

fn is_prefix_namespace(value: &str) -> bool {
    value.ends_with('/') || value.ends_with('#')
}

fn term_targets_equal(left: &TermTarget, right: &TermTarget) -> bool {
    match (left, right) {
        (TermTarget::Absolute(left), TermTarget::Absolute(right)) => left == right,
        (
            TermTarget::Prefixed {
                prefix: left_prefix,
                suffix: left_suffix,
            },
            TermTarget::Prefixed {
                prefix: right_prefix,
                suffix: right_suffix,
            },
        ) => left_prefix == right_prefix && left_suffix == right_suffix,
        (TermTarget::LocalReference(left), TermTarget::LocalReference(right)) => left == right,
        _ => false,
    }
}

fn source(file: &InputContextFile) -> DefinitionSource {
    DefinitionSource {
        path: file.resolved_path.clone(),
    }
}

fn assign_term_const_names<'a>(compacts: impl Iterator<Item = &'a String>) -> syn::Result<HashMap<String, String>> {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for compact in compacts {
        groups.entry(compact.to_shouty_snake_case()).or_default().push(compact.clone());
    }

    let mut result = HashMap::new();
    for (base_name, group) in groups {
        match group.as_slice() {
            [only] => {
                result.insert(only.clone(), base_name);
            }
            [first, second] => {
                let first_is_type = starts_with_uppercase(first);
                let second_is_type = starts_with_uppercase(second);
                if first_is_type != second_is_type {
                    let (type_like, value_like) = if first_is_type { (first, second) } else { (second, first) };
                    result.insert(type_like.clone(), format!("{base_name}_CLASS"));
                    result.insert(value_like.clone(), base_name);
                } else {
                    return Err(syn::Error::new(
                        ProcMacroSpan::call_site().into(),
                        format!("term naming collision: {} and {} both map to {base_name}", first, second),
                    ));
                }
            }
            _ => {
                return Err(syn::Error::new(
                    ProcMacroSpan::call_site().into(),
                    format!("term naming collision: {} compact terms all map to {base_name}", group.join(", ")),
                ));
            }
        }
    }

    Ok(result)
}

fn starts_with_uppercase(value: &str) -> bool {
    value.chars().next().is_some_and(char::is_uppercase)
}

fn path_to_string(path: &Path, span: proc_macro2::Span) -> syn::Result<String> {
    path.to_str()
        .map(ToString::to_string)
        .ok_or_else(|| syn::Error::new(span, format!("path {} is not valid UTF-8", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_http_iri_is_detected() {
        assert!(matches!(
            parse_term_target("description", "http://purl.org/dc/terms/description").unwrap(),
            Some(TermTarget::Absolute(value)) if value == "http://purl.org/dc/terms/description"
        ));
    }

    #[test]
    fn absolute_urn_iri_is_detected() {
        assert!(matches!(
            parse_term_target("nullUri", "urn:ngsi-ld:null").unwrap(),
            Some(TermTarget::Absolute(value)) if value == "urn:ngsi-ld:null"
        ));
    }

    #[test]
    fn prefixed_name_is_detected() {
        assert!(matches!(
            parse_term_target("Property", "ngsi-ld:Property").unwrap(),
            Some(TermTarget::Prefixed { prefix, suffix }) if prefix == "ngsi-ld" && suffix == "Property"
        ));
    }

    #[test]
    fn compact_names_map_to_stable_constants() {
        assert_eq!("createdAt".to_shouty_snake_case(), "CREATED_AT");
        assert_eq!("GeoProperty".to_shouty_snake_case(), "GEO_PROPERTY");
        assert_eq!("ngsi-ld".to_shouty_snake_case(), "NGSI_LD");
        assert_eq!("jsonldContextRel".to_shouty_snake_case(), "JSONLD_CONTEXT_REL");
    }
}
