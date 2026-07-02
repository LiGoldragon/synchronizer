# synchronizer

Meta-repo version propagation: when a low dependency's `main` advances,
wire contracts drift and consumers fail to decode each other. The
synchronizer discovers the dependency DAG from the manifests, computes
what is stale, and cascades the bumps upward from the leaves so the
component tree stays aligned.

Per repository it edits both pin layers as typed data (Cargo.toml +
Cargo.lock via serde; flake.lock as typed JSON, narHash prefetched
through nix), commits and pushes a tool-owned `synchronizer` branch —
never `main` — build-verifies each bump on a role-resolved builder host,
keeps going on failure, and reports the whole run as one NOTA document.

Entrypoint:

- `synchronizer <configuration.nota>` — one NOTA configuration file in,
  one NOTA report out.

Status: design + scaffold. `ARCHITECTURE.md` is the design document
pending psyche sign-off; module bodies are unimplemented.
