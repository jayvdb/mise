use async_trait::async_trait;

use super::{InstallOpts, PackageRequest, PackageStatus, SystemPackageManager, rpm};
use crate::result::Result;
use crate::system::sudo;

/// RedHat-family (Fedora, RHEL, CentOS, Rocky, Alma) via dnf
pub(crate) struct DnfManager {}

impl DnfManager {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

// Never add `--` here: DNF5 rejects it on subcommands like install/upgrade.
// Pins use rpm NEVRA syntax (name-version) which dnf accepts positionally.
fn pkg_operand(p: &PackageRequest) -> String {
    match &p.version {
        Some(v) => format!("{}-{v}", p.name),
        None => p.name.clone(),
    }
}

fn install_args(pkgs: &[PackageRequest], opts: &InstallOpts) -> Vec<String> {
    let mut args = vec!["install".to_string(), "-y".to_string()];
    if opts.update {
        args.push("--refresh".to_string());
    }
    args.extend(pkgs.iter().map(pkg_operand));
    args
}

fn upgrade_args(pkgs: &[PackageRequest]) -> Vec<String> {
    // --refresh: expire cached metadata so "upgrade" actually sees new
    // versions; `dnf upgrade <pkg>` only touches already-installed
    // packages (a pin downgrade would need `dnf install name-version`,
    // which the install path already provides)
    let mut args = vec![
        "upgrade".to_string(),
        "-y".to_string(),
        "--refresh".to_string(),
    ];
    args.extend(pkgs.iter().map(pkg_operand));
    args
}

#[async_trait(?Send)]
impl SystemPackageManager for DnfManager {
    fn name(&self) -> &str {
        "dnf"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux") && crate::file::which("dnf").is_some()
    }

    fn unavailable_reason(&self) -> String {
        if cfg!(target_os = "linux") {
            "dnf not found".to_string()
        } else {
            "only available on linux".to_string()
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        rpm::query(pkgs).await
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let args = install_args(pkgs, opts);
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("dnf", &args).join(" "));
            return Ok(());
        }
        sudo::run("dnf", &args, &[])
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        let args = upgrade_args(pkgs);
        if opts.dry_run {
            miseprintln!("{}", sudo::argv("dnf", &args).join(" "));
            return Ok(());
        }
        sudo::run("dnf", &args, &[])
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
    fn test_install_args_no_separator() {
        let pkgs = vec![req("ripgrep", None), req("bat", Some("0.24.0"))];
        let opts = InstallOpts {
            dry_run: false,
            update: false,
        };
        let args = install_args(&pkgs, &opts);
        // DNF5 rejects a bare `--` on subcommands; it must never appear
        assert!(args.iter().all(|a| a != "--"));
        assert_eq!(args, vec!["install", "-y", "ripgrep", "bat-0.24.0"]);
    }

    #[test]
    fn test_install_args_update_adds_refresh() {
        let pkgs = vec![req("ripgrep", None)];
        let opts = InstallOpts {
            dry_run: false,
            update: true,
        };
        let args = install_args(&pkgs, &opts);
        assert!(args.iter().all(|a| a != "--"));
        // --refresh precedes the operands, after the subcommand flags
        assert_eq!(args, vec!["install", "-y", "--refresh", "ripgrep"]);
    }

    #[test]
    fn test_upgrade_args_no_separator() {
        let pkgs = vec![req("ripgrep", None), req("bat", Some("0.24.0"))];
        let args = upgrade_args(&pkgs);
        assert!(args.iter().all(|a| a != "--"));
        assert_eq!(
            args,
            vec!["upgrade", "-y", "--refresh", "ripgrep", "bat-0.24.0"]
        );
    }
}
