use crate::{
    reference::{
        ReferenceError,
        descriptor::configuration_descriptor,
        key_listing::{ReferenceEntry, key_listing},
        schema_node::SchemaNode,
        template::configuration_template,
    },
    validate::{compound_rules, declared_rules, rule::Rule},
};
use pretty_assertions::assert_eq;
use std::collections::BTreeSet;

/// Every table header and key the template writes, set or commented out, as
/// dotted paths.
fn template_paths(template: &str) -> BTreeSet<String> {
    let mut table = String::new();
    let mut paths = BTreeSet::new();
    for line in template.lines().map(|line| line.strip_prefix("# ").unwrap_or(line)) {
        let header = line.trim_start_matches('[').trim_end_matches(']');
        if line.starts_with('[')
            && header
                .chars()
                .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || "_.".contains(character))
        {
            header.clone_into(&mut table);
            paths.insert(table.clone());
        } else if let Some((key, _)) = line.split_once(" = ")
            && key
                .chars()
                .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_')
        {
            paths.insert(format!("{table}.{key}"));
        }
    }
    paths
}

#[test]
fn the_template_contains_every_leaf_key_in_the_schema() -> Result<(), ReferenceError> {
    let written = template_paths(&configuration_template()?);
    let missing: Vec<String> = SchemaNode::configuration()?
        .leaf_paths("")
        .into_iter()
        .filter(|path| !written.contains(path))
        .collect();
    assert_eq!(missing, [] as [String; 0]);
    Ok(())
}

#[test]
fn no_generated_comment_carries_a_rustdoc_link_or_path() -> Result<(), ReferenceError> {
    let rustdoc: Vec<String> = configuration_template()?
        .lines()
        .filter(|line| line.starts_with('#') && (line.contains("](") || line.contains("Self::") || line.contains("[`")))
        .map(str::to_owned)
        .collect();
    assert_eq!(rustdoc, [] as [String; 0]);
    Ok(())
}

#[test]
fn every_table_and_key_carries_an_operator_description() -> Result<(), ReferenceError> {
    let undescribed: Vec<String> = key_listing()?
        .iter()
        .filter_map(|entry| match entry {
            ReferenceEntry::Table(table) => table.description.is_none().then(|| table.path.dotted()),
            ReferenceEntry::Key(key) => key.description.is_none().then(|| key.path.dotted()),
        })
        .collect();
    assert_eq!(undescribed, [] as [String; 0]);
    Ok(())
}

#[test]
fn a_key_not_set_by_default_is_commented_out_at_its_example() -> Result<(), ReferenceError> {
    let template = configuration_template()?;
    assert!(template.contains("\n# certificate = \"/etc/andromeda/tls/tls.crt\"\n"), "{template}");
    assert!(template.contains("\n# [database.pooler]\n"), "{template}");
    assert!(template.contains("\n# [[jsonld.preloaded_contexts]]\n"), "{template}");
    Ok(())
}

/// A rule names a key by its dotted path, so a path the file does not hold
/// would be a rule the descriptor lists under no key.
#[test]
fn every_rule_names_a_key_of_the_file() -> Result<(), ReferenceError> {
    let paths: BTreeSet<String> = key_listing()?
        .iter()
        .map(|entry| match entry {
            ReferenceEntry::Table(table) => table.path.dotted(),
            ReferenceEntry::Key(key) => key.path.dotted(),
        })
        .collect();
    let unknown: Vec<String> = declared_rules()
        .map(Rule::key)
        .chain(compound_rules().flat_map(|rule| rule.keys.iter().copied()))
        .map(|key| key.to_string())
        .filter(|path| !paths.contains(path))
        .collect();
    assert_eq!(unknown, [] as [String; 0]);
    Ok(())
}

/// The descriptor is what a documentation site prints, so a table, key or
/// choice with no text, such as a value a macro generates, must not reach it.
#[test]
fn the_descriptor_leaves_nothing_undescribed() -> Result<(), ReferenceError> {
    let value = serde_json::to_value(configuration_descriptor()?)?;
    let undescribed: Vec<String> = ["tables", "keys"]
        .into_iter()
        .flat_map(|list| value[list].as_array().into_iter().flatten())
        .flat_map(|item| {
            let path = item["path"].as_str().unwrap_or_default().to_owned();
            let own = item["description"].as_str().is_none_or(|text| text.trim().is_empty()).then(|| path.clone());
            let choices = item["choices"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|choice| choice["description"].as_str().is_none_or(|text| text.trim().is_empty()))
                .map(move |choice| format!("{path} = {}", choice["value"]));
            own.into_iter().chain(choices).collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(undescribed, [] as [String; 0]);
    Ok(())
}
