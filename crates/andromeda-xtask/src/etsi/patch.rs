//! Applying and reverting the ETSI patch sets. Both sets are `git format-patch`
//! output of the `andromeda-corrections` commit series. The infra set bakes the
//! run configuration and the suite-infrastructure corrections into the checkout and
//! is always applied; the corrected-material set adjudicates expectation defects and
//! is applied unless the run asks for the material as-shipped. A guard reverts
//! everything when the run ends, so an inspected checkout is left pristine.

use crate::{error::XtaskError, etsi};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Whether the corrected-material patch set is applied on top of the
/// always-applied infra set.
#[derive(Clone, Copy)]
pub(crate) enum Material {
    /// Apply the corrected-material patches: the default, spec-adjudicated run.
    Corrected,
    /// Skip them, judging the broker against the material exactly as it ships.
    AsShipped,
}

/// Direction a unified diff is applied in.
#[derive(Clone, Copy)]
enum Direction {
    /// Apply the patch.
    Forward,
    /// Reverse a previously applied patch.
    Reverse,
}

/// Applies the selected patches on construction and reverts them, in reverse order,
/// on drop.
pub(crate) struct PatchGuard {
    /// Patch files applied to the checkout, in application order.
    applied: Vec<PathBuf>,
}

impl PatchGuard {
    /// Apply the infra set and, unless the run is as-shipped, the corrected-material
    /// set, recording each for reversal on drop.
    ///
    /// # Errors
    ///
    /// Returns [`XtaskError::PatchApply`] for the first patch that does not apply
    /// cleanly, and an I/O error if a patch directory cannot be read.
    pub(crate) fn apply(material: Material) -> Result<PatchGuard, XtaskError> {
        let mut guard = PatchGuard { applied: Vec::new() };
        for patch in infra_patches()? {
            guard.apply_one(patch)?;
        }
        match material {
            Material::Corrected => {
                for patch in material_patches()? {
                    guard.apply_one(patch)?;
                }
            }
            Material::AsShipped => {}
        }
        Ok(guard)
    }

    /// Apply one patch to the checkout, recording it once it applies cleanly.
    fn apply_one(&mut self, patch: PathBuf) -> Result<(), XtaskError> {
        if git_apply(&patch, Direction::Forward)? {
            self.applied.push(patch);
            Ok(())
        } else {
            Err(XtaskError::PatchApply(patch.display().to_string()))
        }
    }
}

impl Drop for PatchGuard {
    fn drop(&mut self) {
        for patch in self.applied.iter().rev() {
            // Best-effort: the next run hard-resets the checkout regardless, so a
            // failed revert here cannot leak into a subsequent run.
            let _reverted = git_apply(patch, Direction::Reverse);
        }
    }
}

/// The infra patch files (run configuration and suite-infrastructure corrections),
/// sorted by file name so their application order matches the commit series.
fn infra_patches() -> Result<Vec<PathBuf>, XtaskError> {
    patches_in("infra")
}

/// The corrected-material patch files, sorted by file name so their application
/// order is deterministic across platforms.
fn material_patches() -> Result<Vec<PathBuf>, XtaskError> {
    patches_in("corrected")
}

/// Every `.patch` file directly under `suite-delta/<subdir>`, sorted by file name.
/// The `NNNN-` prefix `git format-patch` writes makes lexical order the commit
/// order, so a plain sort reproduces the series' application sequence.
fn patches_in(subdir: &str) -> Result<Vec<PathBuf>, XtaskError> {
    let dir = etsi::resource_dir().join("suite-delta").join(subdir);
    let mut patches: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "patch"))
        .collect();
    patches.sort();
    Ok(patches)
}

/// Apply or reverse one unified diff against the checkout with `git apply`,
/// returning whether it applied cleanly. Both plain and `diff --git` unified diffs
/// are stripped at one leading path component, matching how the patches are
/// authored, and `git apply` leaves the tree untouched when a patch does not apply.
fn git_apply(patch: &Path, direction: Direction) -> Result<bool, XtaskError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(etsi::checkout_dir()).args(["apply", "-p1"]);
    match direction {
        Direction::Forward => {}
        Direction::Reverse => {
            command.arg("--reverse");
        }
    }
    let status = command.arg(patch).status()?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use crate::etsi::patch::{infra_patches, material_patches};

    #[test]
    fn the_two_infra_patches_ship_in_sorted_order() {
        let patches = infra_patches().expect("the infra patch directory is readable");
        // The two infra commits: run configuration then the csr-endpoint correction.
        assert_eq!(patches.len(), 2);
        assert!(patches.iter().all(|path| path.extension().is_some_and(|extension| extension == "patch")));
        assert!(patches.is_sorted());
    }

    #[test]
    fn every_material_patch_is_a_dot_patch_file_in_sorted_order() {
        let patches = material_patches().expect("the patch directory is readable");
        // The crate ships exactly the corrected-material set the matrix records.
        assert_eq!(patches.len(), 66);
        assert!(patches.iter().all(|path| path.extension().is_some_and(|extension| extension == "patch")));
        assert!(patches.is_sorted());
    }
}
