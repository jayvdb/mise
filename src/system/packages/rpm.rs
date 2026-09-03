//! Shared rpm queries for the RPM-family managers (dnf, zypper).
//!
//! Installed state comes from the local rpm database rather than the package
//! manager itself: it is the same answer both managers would give, it is
//! read-only, and it never elevates.

use std::collections::HashMap;
use std::process::Stdio;

use eyre::bail;

use super::{PackageRequest, PackageState, PackageStatus};
use crate::result::Result;

/// `%{NAME}\t%{VERSION}-%{RELEASE}` lines from `rpm -q`, matched positionally
/// back onto `requests`.
pub(super) fn parse_query(output: &str, requests: &[PackageRequest]) -> Vec<PackageStatus> {
    let mut installed: HashMap<&str, &str> = HashMap::new();
    for line in output.lines() {
        if let Some((name, version)) = line.split_once('\t') {
            installed.insert(name, version);
        }
    }
    requests
        .iter()
        .map(|req| {
            let state = match installed.get(req.name.as_str()) {
                // a pin must match the installed version-release exactly, or
                // its version part (a version-only pin matches any release)
                Some(version) => match &req.version {
                    Some(requested)
                        if *version != requested
                            && !version.starts_with(&format!("{requested}-")) =>
                    {
                        PackageState::VersionMismatch {
                            installed: version.to_string(),
                        }
                    }
                    _ => PackageState::Installed {
                        version: version.to_string(),
                    },
                },
                None => PackageState::Missing,
            };
            PackageStatus {
                request: req.clone(),
                state,
            }
        })
        .collect()
}

/// Query the local rpm database for the given packages.
pub(super) async fn query(pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
    if pkgs.is_empty() {
        return Ok(vec![]);
    }
    let mut args = vec![
        "-q".to_string(),
        "--qf".to_string(),
        "%{NAME}\\t%{VERSION}-%{RELEASE}\\n".to_string(),
    ];
    args.extend(pkgs.iter().map(|p| p.name.clone()));
    debug!("$ rpm {}", args.join(" "));
    let output = tokio::process::Command::new("rpm")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    // rpm -q exits nonzero when any package is not installed; "package X
    // is not installed" goes to stdout or stderr depending on rpm version
    // and won't match the \t format either way — absent packages parse as
    // Missing. Only fail on rpm errors unrelated to missing packages.
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success()
        && !stderr.is_empty()
        && !stderr.lines().all(|l| {
            l.trim().is_empty() || l.contains("is not installed") || l.contains("no packages")
        })
    {
        bail!("rpm -q failed: {}", stderr.trim());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_query(&stdout, pkgs))
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
    fn test_parse_query() {
        let requests = vec![
            req("bc", None),
            req("nonexistent", None),
            req("bash", Some("5.2.26-3.fc40")),
            req("glib2-devel", None),
            req("zsh", Some("5.8-1.fc40")),
            req("tmux", Some("3.4")),
        ];
        let output = "bc\t1.07.1-14.fc39\npackage nonexistent is not installed\nbash\t5.2.26-3.fc40\nglib2\t2.80.0-1.fc40\nzsh\t5.9-2.fc40\ntmux\t3.4-3.fc40\n";
        let statuses = parse_query(output, &requests);
        assert_eq!(
            statuses[0].state,
            PackageState::Installed {
                version: "1.07.1-14.fc39".to_string()
            }
        );
        assert_eq!(statuses[1].state, PackageState::Missing);
        // a version-release pin matches the full installed version-release
        assert_eq!(
            statuses[2].state,
            PackageState::Installed {
                version: "5.2.26-3.fc40".to_string()
            }
        );
        // an installed "glib2" must not satisfy a "glib2-devel" request
        assert_eq!(statuses[3].state, PackageState::Missing);
        // a different installed version must not satisfy a pin
        assert_eq!(
            statuses[4].state,
            PackageState::VersionMismatch {
                installed: "5.9-2.fc40".to_string()
            }
        );
        // a version-only pin matches any release
        assert_eq!(
            statuses[5].state,
            PackageState::Installed {
                version: "3.4-3.fc40".to_string()
            }
        );
    }
}
