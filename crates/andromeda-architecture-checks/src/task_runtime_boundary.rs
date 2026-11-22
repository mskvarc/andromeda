//! The task runtime runs tasks on an executor it is handed, and the policy
//! crates never decide how their work runs.
//!
//! `andromeda-task-runtime` supervises named tasks on the Tokio executor the
//! broker binary builds, so its production code builds none of its own. The
//! composition adapts policy operations and adapters into the runtime's
//! execution forms, so only the crates that compose the broker's tasks depend
//! on it, and a policy crate names none of its task types and no Tokio task
//! handle.

use crate::{
    dependencies::{declarations, workspace_manifest_paths},
    source_scan::{PACKAGE_DIRECTORIES, item_is_test_only, workspace_sources},
    workspace_file::read_workspace_file,
};
use std::path::Path;
use syn::{
    File,
    Ident,
    Item,
    visit::{Visit, visit_ident, visit_item, visit_path},
};

/// The package that runs the broker's tasks.
const TASK_RUNTIME: &str = "andromeda-task-runtime";

/// The packages that compose the broker's tasks, the only ones that depend on
/// the task runtime.
const COMPOSING_PACKAGES: &[&str] = &["andromeda-broker-composition", "andromeda-tenancy"];

/// The policy packages, whose operations the composition adapts into tasks.
const POLICY_PACKAGES: &[&str] = &[
    "andromeda-context-registry",
    "andromeda-entity",
    "andromeda-entity-map",
    "andromeda-federation",
    "andromeda-notification",
    "andromeda-registration",
    "andromeda-snapshot",
    "andromeda-subscription",
    "andromeda-temporal",
];

/// The types that register, supervise or hold a running task.
const TASK_TYPES: &[&str] = &["JoinHandle", "TaskRegistration", "TaskSupervisor"];

/// The calls that build a Tokio executor, as the last two segments of their
/// path.
const EXECUTOR_BUILDERS: &[[&str; 2]] = &[["Runtime", "new"], ["Builder", "new_multi_thread"], ["Builder", "new_current_thread"]];

/// Assert that only the composing packages depend on the task runtime, that the
/// task runtime builds no executor, and that no policy package names a task
/// type.
///
/// # Panics
///
/// Panics when a rule is broken, or when a manifest or a source cannot be read
/// or parsed.
pub fn assert_task_runtime_boundary(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    for path in workspace_manifest_paths(root) {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        if let Some(manifest) = read_workspace_file(root, &relative, &mut violations) {
            violations.extend(refused_dependency(&relative, &manifest));
        }
    }
    for source in workspace_sources(root) {
        violations.extend(source_violations(&source.workspace_path, &source.syntax));
    }
    assert!(
        violations.is_empty(),
        "the task runtime boundary is crossed; the composition adapts policy operations into tasks, and the task runtime runs them on the executor it is handed:\n{}",
        violations.join("\n")
    );
}

/// The package a workspace-relative path lies in, named by its directory below
/// a package directory; `None` for a path of the root package.
fn package_of(workspace_path: &str) -> Option<&str> {
    let (directory, rest) = workspace_path.split_once('/')?;
    let package = rest.split('/').next()?;
    PACKAGE_DIRECTORIES.contains(&directory).then_some(package)
}

/// Every declaration of the task runtime in the manifest at `relative`, unless
/// it is the manifest of a composing package.
fn refused_dependency(relative: &str, manifest: &str) -> Vec<String> {
    if package_of(relative).is_some_and(|package| COMPOSING_PACKAGES.contains(&package)) {
        return Vec::new();
    }
    declarations(manifest)
        .into_iter()
        .filter(|(_, name)| *name == TASK_RUNTIME)
        .map(|(kind, _)| format!("{relative} declares {TASK_RUNTIME} as a {kind:?} dependency"))
        .collect()
}

/// The violations in the production source at `workspace_path`: an executor
/// the task runtime builds, or a task type a policy package names.
fn source_violations(workspace_path: &str, syntax: &File) -> Vec<String> {
    let mut visitor = match package_of(workspace_path) {
        Some(TASK_RUNTIME) => BoundaryVisitor::new(workspace_path, Rule::BuildsNoExecutor),
        Some(package) if POLICY_PACKAGES.contains(&package) => BoundaryVisitor::new(workspace_path, Rule::NamesNoTaskType),
        Some(_) | None => return Vec::new(),
    };
    visitor.visit_file(syntax);
    visitor.violations
}

/// The rule a package's production source is held to.
#[derive(Debug, Clone, Copy)]
enum Rule {
    /// The task runtime builds no Tokio executor.
    BuildsNoExecutor,
    /// A policy package names no task type.
    NamesNoTaskType,
}

/// Finds what one production source of a held package breaks its rule with,
/// outside test-only items.
struct BoundaryVisitor<'path> {
    /// Workspace-relative path of the source.
    path: &'path str,
    /// The rule the source is held to.
    rule: Rule,
    /// What the source breaks the rule with.
    violations: Vec<String>,
}

impl BoundaryVisitor<'_> {
    /// A visitor holding the source at `path` to `rule`, with nothing found yet.
    const fn new(path: &str, rule: Rule) -> BoundaryVisitor<'_> {
        BoundaryVisitor {
            path,
            rule,
            violations: Vec::new(),
        }
    }
}

impl<'ast> Visit<'ast> for BoundaryVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if matches!(self.rule, Rule::BuildsNoExecutor) {
            let segments = path.segments.iter().map(|segment| segment.ident.to_string()).collect::<Vec<_>>();
            if let [.., owner, call] = segments.as_slice()
                && EXECUTOR_BUILDERS
                    .iter()
                    .any(|[builder_owner, builder_call]| builder_owner == owner && builder_call == call)
            {
                self.violations.push(format!(
                    "{} builds a Tokio executor with {owner}::{call}; take the executor the binary built",
                    self.path
                ));
            }
        }
        visit_path(self, path);
    }

    fn visit_ident(&mut self, identifier: &'ast Ident) {
        if matches!(self.rule, Rule::NamesNoTaskType)
            && let Some(task_type) = TASK_TYPES.iter().find(|task_type| identifier == *task_type)
        {
            self.violations.push(format!(
                "{} names {task_type}; the composition adapts the policy operation into a task",
                self.path
            ));
        }
        visit_ident(self, identifier);
    }
}

#[cfg(test)]
mod tests {
    use crate::task_runtime_boundary::{package_of, refused_dependency, source_violations};
    use pretty_assertions::assert_eq;

    fn violations(workspace_path: &str, source: &str) -> Vec<String> {
        source_violations(workspace_path, &syn::parse_file(source).unwrap())
    }

    #[test]
    fn a_path_below_a_package_directory_lies_in_that_package() {
        assert_eq!(package_of("crates/andromeda-task-runtime/src/supervisor.rs"), Some("andromeda-task-runtime"));
        assert_eq!(package_of("tools/andromeda-sql/Cargo.toml"), Some("andromeda-sql"));
        assert_eq!(package_of("src/main.rs"), None);
        assert_eq!(package_of("Cargo.toml"), None);
    }

    #[test]
    fn a_composing_package_may_depend_on_the_task_runtime() {
        let manifest = "[dependencies]\nandromeda-task-runtime.workspace = true\n";
        assert_eq!(refused_dependency("crates/andromeda-tenancy/Cargo.toml", manifest), [] as [String; 0]);
        assert_eq!(
            refused_dependency("crates/andromeda-broker-composition/Cargo.toml", manifest),
            [] as [String; 0]
        );
    }

    #[test]
    fn any_other_package_depending_on_the_task_runtime_in_any_table_is_refused() {
        let manifest = "[dependencies]\nandromeda-task-runtime.workspace = true\n[dev-dependencies]\nandromeda-task-runtime.workspace = true\n";
        assert_eq!(
            refused_dependency("crates/andromeda-entity/Cargo.toml", manifest),
            [
                "crates/andromeda-entity/Cargo.toml declares andromeda-task-runtime as a Normal dependency",
                "crates/andromeda-entity/Cargo.toml declares andromeda-task-runtime as a Development dependency",
            ]
        );
    }

    #[test]
    fn the_workspace_declaration_of_the_task_runtime_is_no_dependency() {
        let manifest = "[workspace.dependencies]\nandromeda-task-runtime = { path = \"crates/andromeda-task-runtime\" }\n";
        assert_eq!(refused_dependency("Cargo.toml", manifest), [] as [String; 0]);
    }

    #[test]
    fn the_task_runtime_building_an_executor_is_refused() {
        for source in [
            "fn host() { let runtime = tokio::runtime::Runtime::new(); }",
            "fn host() { let runtime = Builder::new_multi_thread().build(); }",
            "fn host() { let runtime = Builder::new_current_thread().build(); }",
        ] {
            assert_eq!(violations("crates/andromeda-task-runtime/src/supervisor.rs", source).len(), 1, "{source}");
        }
    }

    #[test]
    fn the_task_runtime_taking_a_handle_or_building_an_executor_in_a_test_passes() {
        let source = "fn spawn(handle: &Handle) { handle.spawn(work()); }\n#[cfg(test)] mod tests { fn host() { Builder::new_current_thread().build(); } }";
        assert_eq!(violations("crates/andromeda-task-runtime/src/supervisor.rs", source), [] as [String; 0]);
    }

    #[test]
    fn a_policy_package_naming_a_task_type_is_refused() {
        for source in [
            "use tokio::task::JoinHandle;",
            "struct Drain { handle: tokio::task::JoinHandle<()> }",
            "fn register(registration: TaskRegistration) {}",
            "fn supervise(supervisor: &TaskSupervisor) {}",
        ] {
            assert_eq!(violations("crates/andromeda-entity/src/drain.rs", source).len(), 1, "{source}");
        }
    }

    #[test]
    fn a_package_outside_the_rules_may_build_an_executor_and_name_task_types() {
        let source = "use tokio::task::JoinHandle; fn host() { Builder::new_current_thread().build(); }";
        assert_eq!(violations("crates/andromeda-delivery/src/worker.rs", source), [] as [String; 0]);
        assert_eq!(violations("src/main.rs", source), [] as [String; 0]);
    }
}
