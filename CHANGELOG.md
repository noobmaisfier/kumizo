# Changelog

Notable changes to the packages published from this repository:
`kumizo-kit` (SDK), `kumizo` (CLI) and `kumizo-aurora` (reference shell).

## 0.11.1 — 2026-10-25

Documentation-only release: no API, behaviour or dependency change, drop-in
upgrade from 0.11.0.

### Changed

- `kumizo-kit` and `kumizo` now publish the repository README on crates.io
  (they previously packaged `readme = false`); licence and guide links in it
  are absolute so they resolve outside the repository.
- `kumizo-kit` crate docs: the feature table now lists `full`.
- `aurora/README.md`: install section rewritten around `cargo install`, build
  prerequisites spelled out.

### Removed

- **Binary install scheme.** `install-aurora.sh` is deleted, along with the
  GitHub/GitLab jobs that produced and published `kumizo-linux-x86_64` /
  `aurora-*-linux-x86_64` tarballs and the GitHub/GitLab releases. CI is now a
  single `check` job (workspace check + lib tests). There are no pre-built
  binaries: `cargo install --locked kumizo-aurora kumizo` (crates.io) or
  `cargo build` from the repository are the only supported install paths.
  This affects the repository, not the published crates.

## 0.11.0 — 2026-10-25

### Changed

- **Three packages instead of nine.** `wk-ui`, `wk-gpui-ext`, `wk-services`,
  `wk-compositor`, `wk-ipc` and `wk-pam` are now modules of `kumizo-kit`
  (`kumizo_kit::{ui,ext,services,compositor,ipc,pam}`), each behind a feature
  flag. Repository layout: `kumizo-kit/` (SDK), `kumizo-cli/` (package
  `kumizo`), `aurora/` (package `kumizo-aurora`).
- Features: `app` (default; implies every module), `ipc`, `ext`, `ui`,
  `services`, `compositor`, `pam`, `layer-shell` (default), `full` (alias of
  the defaults today).
- The `kumizo` CLI depends on `kumizo-kit` with `default-features = false,
  features = ["ipc"]`: no GPUI, wgpu or zbus in its build.

### Added

- `kumizo-aurora` package, shipping the `aurora` and `aurora-lockscreen`
  binaries under their original names.

### Removed

- **Binary install scheme.** `install-aurora.sh` is deleted, along with the
  GitHub/GitLab jobs that produced and published `kumizo-linux-x86_64` /
  `aurora-*-linux-x86_64` tarballs and the GitHub/GitLab releases. CI is now a
  single `check` job (workspace check + lib tests). There are no pre-built
  binaries: `cargo install --locked kumizo-aurora kumizo` (crates.io) or
  `cargo build` from the repository are the only supported install paths.

### Yanked

- `wk-ui`, `wk-gpui-ext`, `wk-services`, `wk-compositor`, `wk-ipc`, `wk-pam`
  **0.1.0** and `kumizo-kit` **0.1.0** are yanked on crates.io. Existing
  `Cargo.lock` files still build them; new projects must use `kumizo-kit`
  0.11.0 or later. Reversible with `cargo yank --undo --version 0.1.0 <crate>`.

---

- `gpui-pre` is pinned to `=0.3.9` (via `gpui-pre-platform` for the platform
  bootstrap). Any update is a documented minor event in this file.
