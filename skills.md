# skills — synchronizer

*Per-repo agent guide.*

## Checkpoint — read before editing

- this repo's `ARCHITECTURE.md` — the design document; the locked design
  decisions in it are psyche-decided and are not relitigated in code;
- workspace skills: `nota-design`, `nota-schema-docs`, `rust-crate-layout`,
  `rust-errors`, `typed-records-over-flags`, `abstractions`, `naming`.

## Repo-specific rules

- Implemented against the psyche-signed design; `ARCHITECTURE.md` §14
  records the resolved sign-off decisions, two of them psyche-locked
  (format-preserving TOML writes, wire-exercising verify). Do not
  relitigate them in code.
- All manifest and lock manipulation is typed (serde reads, the
  format-preserving `toml_edit` document for TOML writes, the canonical
  NOTA codec, winnow); no string munging. The single sanctioned narrow
  text edit is the flake.nix input-URL literal substitution described in
  `src/flake_manifest.rs`.
- The tool never writes `main` and never touches a working copy; see the
  invariants in `ARCHITECTURE.md` §13 before adding any git operation.
- No hostname anywhere in source or configuration; builder selection goes
  through the `ClusterRoleDirectory` boundary only.
- Do not run the tool against the real LiGoldragon component repositories
  from a development session; exercise it on fixtures. The first live run
  is a deliberate, psyche-directed step.
