# synchronizer

Meta-repo version propagation: when a low dependency's `main` advances,
wire contracts drift and consumers fail to decode each other. The
synchronizer discovers the dependency DAG from the manifests, computes
what is stale, and cascades the bumps upward from the leaves so the
component tree stays aligned.

Per repository it edits both pin layers as typed, format-preserving data
(Cargo.toml + Cargo.lock through a comment-preserving TOML document, serde
for reading; flake.lock as typed JSON, narHash prefetched through nix),
builds each bump commit at the git-object level — no working copy is
touched — and pushes a tool-owned staging branch, never the mainline. Each
pushed bump is verified on a configured builder host with the repository's
wire-exercising flake checks (the daemon-launching class that catches
runtime wire skew), falling back to the default `nix build` where a repo has
none. Failures are collected, never fatal; the whole run is reported as one
NOTA document.

The tool carries zero project data. Every project-specific fact — forge,
components, branch scheme, builder-host strategy (a directly named host or a
role resolved through a cluster directory), verify-gate words, and commit
author — lives in a typed NOTA configuration. Anyone runs it against their
own repositories by writing their own config; the CriomOS cluster-datom
builder resolver is one optional plugin, never assumed. See `ARCHITECTURE.md`
§0a and §3 for the design law and full config schema.

Entrypoint:

- `synchronizer <configuration.nota>` — one NOTA configuration file in,
  one NOTA report out; exit 1 when the report carries failures.
- `cargo run --example validate -- <configuration.nota>` — decode-only
  config check; performs no git, nix, or network operation.

## Epic release trains

`release-trains/<name>.nota` is a separate authored intent surface; it does
not extend operational synchronizer configuration or replace Cargo/flake
locks. A train resolves branch selectors to immutable commits, validates that
manifest-discovered topology stays within its declared component set, requires
an exact expected base for each component, and permits external components
only when their exact immutable commit is explicitly admitted.

The typed `release_train` module emits a domain-separated closure identity,
canonical `release-train.lock.json`, component-local lock identities, and an
integration flake that contains only `github:<owner>/<repo>/<commit>` plus
`narHash` sources. The source/vendor seam is data-only until measured cache
reuse justifies a separate immutable index. See `release-trains/README.md` and
`ARCHITECTURE.md` §15.

Status: implemented against the psyche-signed `ARCHITECTURE.md`, including
the universality refactor (all project data externalized to config); not yet
run against live component repositories.
