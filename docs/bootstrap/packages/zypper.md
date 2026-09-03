# zypper

System packages for SUSE-family Linux (openSUSE Leap, openSUSE Tumbleweed,
SLES, ...).

```toml
[bootstrap.packages]
"zypper:libopenssl-devel" = "latest"
"zypper:postgresql-server" = "latest"
"zypper:bash" = "5.2.15" # version or version-release pin
"zypper:PackageKit" = { state = "absent" }
```

## Behavior

- Package state is checked with `rpm -q` (read-only, never elevates), the same
  local database `zypper` itself resolves against.
- Missing packages are installed with `zypper --non-interactive install`,
  elevated with sudo when necessary (see [sudo](/bootstrap/packages/#sudo)).
- Version pins are passed to zypper as its native `name=version` capability
  syntax; a version-only pin is satisfied by any release of that version. When
  anything in the batch is pinned, mise adds `--oldpackage` so a pin can move a
  package backwards from a newer installed version.
- `mise bootstrap packages apply --update` runs `zypper --non-interactive refresh`
  first; otherwise the repositories' own autorefresh setting decides.
- `mise bootstrap packages upgrade` refreshes and then re-runs
  `zypper --non-interactive install` for the configured packages. `install` is
  used rather than `zypper update` because it is the invocation that can honor
  a version pin; for an unpinned entry it resolves to the best available
  candidate, which is an upgrade when a newer one exists and a no-op otherwise.
  Only already-installed packages are touched.
- Entries may set `state = "absent"` to be removed with
  `zypper --non-interactive remove`.

::: info
zypper signals some successful transactions with a non-zero exit code — 102
(reboot required), 103 (zypper updated itself and wants to be restarted), and
106 (a repository was unreachable and skipped). mise treats those three as
success; every other non-zero code, including 104 (package not found), fails.
:::

::: warning
mise does not pass `--auto-agree-with-licenses`. A package that requires
accepting a license agreement (some fonts and firmware) will not install
non-interactively — install it by hand once and mise will see it as present.
:::
