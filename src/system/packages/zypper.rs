use async_trait::async_trait;

use super::{InstallOpts, PackageRequest, PackageStatus, SystemPackageManager, rpm};
use crate::result::Result;
use crate::system::sudo;

/// SUSE-family (openSUSE Leap/Tumbleweed, SLES) via zypper
pub(crate) struct ZypperManager {}

impl ZypperManager {
    pub(crate) fn new() -> Self {
        Self {}
    }

    fn refresh(&self, opts: &InstallOpts) -> Result<()> {
        run(
            &["--non-interactive".to_string(), "refresh".to_string()],
            opts,
        )
    }
}

/// Exit codes that mean the transaction committed even though zypper did not
/// return 0. All three are informational: the machine needs a reboot for the
/// new package to take effect (102), zypper updated itself and wants to be
/// restarted (103), or a repository was unreachable and skipped (106) — a
/// package that could not be found at all fails with 104 instead.
const INFORMATIONAL_EXIT_CODES: &[i32] = &[102, 103, 106];

fn run(args: &[String], opts: &InstallOpts) -> Result<()> {
    if opts.dry_run {
        miseprintln!("{}", sudo::argv("zypper", args).join(" "));
        return Ok(());
    }
    sudo::run_with_ok_codes("zypper", args, &[], INFORMATIONAL_EXIT_CODES)
}

// No `--` separator: zypper takes capability strings positionally and a
// version pin renders to its native `name=version` capability syntax, which
// `install` resolves against the repositories.
fn pkg_operand(p: &PackageRequest) -> String {
    match &p.version {
        Some(v) => format!("{}={v}", p.name),
        None => p.name.clone(),
    }
}

fn install_args(pkgs: &[PackageRequest]) -> Vec<String> {
    let mut args = vec!["--non-interactive".to_string(), "install".to_string()];
    // zypper refuses to replace an installed package with an older one unless
    // asked. Only relax that when something is actually pinned, so an
    // unpinned entry can never silently move backwards.
    if pkgs.iter().any(|p| p.version.is_some()) {
        args.push("--oldpackage".to_string());
    }
    args.extend(pkgs.iter().map(pkg_operand));
    args
}

fn remove_args(pkgs: &[PackageRequest]) -> Vec<String> {
    let mut args = vec!["--non-interactive".to_string(), "remove".to_string()];
    // the pinned version is irrelevant to a removal — only one version of an
    // rpm package is installed at a time
    args.extend(pkgs.iter().map(|p| p.name.clone()));
    args
}

#[async_trait(?Send)]
impl SystemPackageManager for ZypperManager {
    fn name(&self) -> &str {
        "zypper"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("zypper").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "zypper not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        rpm::query(pkgs).await
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if opts.update {
            self.refresh(opts)?;
        }
        run(&install_args(pkgs), opts)
    }

    fn supports_remove(&self) -> bool {
        true
    }

    async fn remove(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        run(&remove_args(pkgs), opts)
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        // Refresh first so the newest candidates are visible, then reuse
        // `install`: unlike `zypper update`, it can move a package to an
        // explicitly pinned version, and for an unpinned entry it resolves to
        // the best available candidate — an upgrade when a newer one exists
        // and a no-op otherwise.
        self.refresh(opts)?;
        run(&install_args(pkgs), opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(name: &str, version: Option<&str>) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: version.map(str::to_string),
            tap_url: None,
            desired: crate::system::packages::PackageDesiredState::Present,
        }
    }

    #[test]
    fn test_install_args_unpinned() {
        let args = install_args(&[req("ripgrep", None), req("git-core", None)]);
        assert_eq!(
            args,
            vec!["--non-interactive", "install", "ripgrep", "git-core"]
        );
    }

    #[test]
    fn test_install_args_pin_uses_capability_syntax() {
        let args = install_args(&[req("ripgrep", None), req("bash", Some("5.2.15-150500.7.1"))]);
        assert_eq!(
            args,
            vec![
                "--non-interactive",
                "install",
                // a pin may need to go backwards from what is installed
                "--oldpackage",
                "ripgrep",
                "bash=5.2.15-150500.7.1",
            ]
        );
    }

    #[test]
    fn test_remove_args_drop_the_pin() {
        let args = remove_args(&[req("bash", Some("5.2.15-150500.7.1")), req("tmux", None)]);
        assert_eq!(args, vec!["--non-interactive", "remove", "bash", "tmux"]);
    }
}
