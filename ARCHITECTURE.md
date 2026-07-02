# synchronizer — architecture

*Meta-repo version propagation: cascade dependency bumps across the
component tree so wire contracts stay aligned.*

This document is the concrete design for psyche sign-off. Sections 1–13
render the locked design; §14 lists the open questions the psyche should
resolve before implementation. The repo is scaffold only: module stubs with
typed signatures, no logic.

## 0. Intent

Direction, backed by psyche statements:

- Mechanical cross-repo version propagation must be a **tool**, not agent
  hand-work.
- The failure this tool kills: when a low dependency's `main` advances, wire
  contracts drift and consumers fail to decode each other. This is the
  witnessed build-vintage wire-skew class — rkyv decode failures surfacing
  as misleading typed symptoms on the first bring-up attempt (kink #2 of
  `~/primary/reports/field-readiness/02-kink-ledger.md`; tracker carriers
  primary-w46v lock-sync sweep, primary-mddx build-vintage fingerprint,
  primary-95fm pin convergence). The synchronizer is the durable fix for
  that whole class: instead of sweeping locks by hand after skew is hit, the
  tool cascades the bumps so the tree stays aligned before daemons meet at
  the wire.

The following design decisions are psyche-decided and locked. Implementation
refines them; it does not relitigate them:

1. configuration is NOTA;
2. both pin layers are managed — Cargo (Cargo.toml + Cargo.lock) and flake
   (flake.lock, plus flake.nix where the pin lives in the URL) — as typed
   data, never string munging;
3. topology is discovered from the manifests, not declared in configuration;
4. version resolution is cascade-aware: unchanged dependencies target the
   latest pushed `main` tip, dependencies bumped this run target their
   pushed `synchronizer` branch tip;
5. every modified repo gets a commit pushed to a dedicated `synchronizer`
   branch — never `main`;
6. every bump is build-verified on a builder whose host is resolved from a
   CriomOS role — no hostname anywhere in the tool;
7. on failure the run keeps going, collects all failures, and reports them
   together; pushed-but-broken synchronizer branches are accepted;
8. the run's output is a NOTA report;
9. the algorithm is a self-driving topological ascent from the leaves.

## 1. Non-goals

- **Never `main`.** The tool never edits, commits to, or pushes `main` of
  any repository. Landing synchronizer branches on `main` is separate,
  deliberate integration work outside this tool.
- **Not a daemon, not CI.** One run is one process: read config, act,
  report, exit. There is no durable tool state — the repositories and the
  emitted report are the only state — so the workspace daemon-first rule
  does not bind here: nothing needs supervision, subscriptions, actors, or
  a shared runtime. If the tool ever grows durable state or a scheduled
  loop, it becomes a daemon + thin CLI per workspace shape at that point.
- **No merging.** The tool never merges synchronizer branches anywhere.
- **Component pins only.** Third-party inputs (nixpkgs, fenix, crane) and
  registry crate versions are not bumped; only pins between configured
  components move (candidate extension, §14 q7).
- **No working-copy contact.** The configured checkouts are used as git
  object stores and push origins only; working copies, indexes, current
  branches, and agent-owned branches are never read or written.

## 2. Component shape

One Rust crate in its own repository: `synchronizer` library plus one thin
`synchronizer` binary (`fn main` only). The binary takes exactly one
argument — the NOTA configuration file path — prints the NOTA report to
stdout, and exits nonzero when the run was fatal or the report carries
failures.

## 3. Configuration (NOTA)

The configuration names the participants and where their clones live. It
never declares dependency edges (§5) and never names a host (§8). Decoding
goes only through the canonical NOTA codec (Spirit record qvb3); the Rust
schema in `src/configuration.rs` plus round-trip tests own the wire truth —
the pseudo-NOTA here is documentation.

```nota
;; SynchronizerConfig: root configuration document (strict positional).
(SynchronizerConfig <forge> <checkout-root> [<component>] <builder-role> <cluster-configuration>)
;;   forge                : (GitHub <owner>) — forge account holding every component remote
;;   checkout-root        : absolute path holding local clones named after components
;;   component            : (Component <name> <checkout>)
;;   builder-role         : bare atom — CriomOS role of the build-verify host
;;   cluster-configuration: (ClusterFlake <flake-reference>) — where roles resolve to hosts

;; Component: one participating repository.
(Component <name> <checkout>)
;;   name    : bare atom — the GitHub repository name, e.g. signal-router
;;   checkout: AtRoot | (AtPath <absolute-path>)
;;     AtRoot   — the clone lives at <checkout-root>/<name> (ghq-style layout)
;;     (AtPath <absolute-path>) — explicit override
```

Example:

```nota
(SynchronizerConfig
  (GitHub LiGoldragon)
  /git/github.com/LiGoldragon
  [(Component signal-frame AtRoot)
   (Component signal-router AtRoot)
   (Component signal-harness AtRoot)
   (Component introspect AtRoot)]
  Builder
  (ClusterFlake [github:LiGoldragon/CriomOS-test-cluster]))
```

Notes:

- Field order is the positional wire contract; growth happens by trailing
  field or new variant, never by reordering.
- `checkout` is a closed enum with a required payload on the override
  variant — no optional in a positional slot.
- The dedicated branch name `synchronizer` is a design constant, not
  configuration (`BranchName::synchronizer()`).
- The `cluster-configuration` variant set is expected to firm up when the
  concrete cluster surface is confirmed (§14 q5).

## 4. Pin layers

A dependency between two components is pinned in up to four places, and
each is a typed model:

### Cargo layer

- **`Cargo.toml`** (`src/cargo_manifest.rs`) — serde through the TOML
  deserializer into `CargoManifest`; the dependency tables the tool
  manipulates are typed, everything else is preserved as typed TOML values
  (`#[serde(flatten)] remainder: toml::Table`) and reserialized untouched.
  In this workspace sibling dependencies are declared
  `{ git = "https://github.com/LiGoldragon/<repo>.git", branch = "main" }`,
  so the manifest changes **only** on a cascade pin: the dependency is
  redirected `branch = "main"` → `branch = "synchronizer"` so Cargo can
  reach the locked revision from a fresh clone (a rev locked off-branch is
  unreachable through a `branch = "main"` declaration).
- **`Cargo.lock`** (`src/cargo_lock.rs`) — serde-typed `CargoLock`. A bump
  sets the locked revision inside the parsed `source` pin (winnow parser
  over the `git+<url>?<query>#<rev>` shape — never ad hoc splitting),
  rewrites the reference query to match the manifest, and synchronizes the
  recorded package `version` to what the dependency's own manifest declares
  at the target revision (read through the dependency's object store, §7).
- **Package name vs repository name.** A Cargo dependency key is a package
  name and may differ from the repository name (`nota` lives in
  `nota-next`). All component matching goes through the git URL's
  repository identity, never the package name.
- **Pretty printing** (`src/toml_pretty.rs`) — reserialization goes through
  `PrettyPrinter`, which owns the canonical output policy: the workspace
  manifest style (aligned `=` within dependency tables) and Cargo's own
  lock rendering including its `@generated` header. It renders typed TOML
  values only; it never patches text it did not produce.
- **Known consequences, for sign-off:** serde reserialization of a
  `Cargo.toml` drops TOML comments (§14 q1), and a typed lock edit cannot
  invent new transitive entries when the dependency's own dependency set
  changed at the target revision — build-verify catches that case and
  reports it (§14 q2).

### Flake layer

- **`flake.lock`** (`src/flake_lock.rs`) — typed JSON via serde
  (`FlakeLock`, `LockNode`, `LockedSource`, `OriginalSource`; unknown
  fields preserved through typed remainders). A bump sets `locked.rev`
  in-type, obtains `narHash` and `lastModified` through the
  `NarHashSource` boundary — `nix flake prefetch --json`, the **only**
  external command in the whole pin-editing path, because the narHash is
  the one value text manipulation cannot produce — and, on a cascade pin,
  sets `original.ref` to `synchronizer` so a later `nix flake update`
  follows the same branch the lock points into. Reserialization is Nix's
  canonical lock rendering (two-space indent, sorted keys).
- **`flake.nix`** (`src/flake_manifest.rs`) — edited only where a pin
  lives in the input URL itself (`github:owner/repo/<rev-or-ref>`) rather
  than in the lock. The URL literal is located and parsed by a winnow
  scanner into a typed `InputUrl`, rewritten in-type, and substituted back
  at its recorded byte span. The tool does not model Nix source beyond
  input URL literals; this span substitution is the single sanctioned
  narrow text edit in the design.

## 5. Topology — discovered, not declared

`src/topology.rs`. The configuration provides the component *set*; the
*edges* come only from the manifests read at each component's remote `main`
tip:

- Cargo git dependencies (across `dependencies`, `build-dependencies`,
  `dev-dependencies`) whose repository URL matches a configured component;
- flake inputs whose locked GitHub source matches a configured component by
  owner and repository.

Each match is a `DependencyEdge { consumer, producer, layer }`; a consumer
typically holds two edges to the same producer (`CargoLock` + `FlakeLock`),
each bumped and reported independently. Anything pointing outside the
configured set produces no edge. The result must be a DAG; a cycle is
run-fatal (`Error::DependencyCycle`) because it admits no ascent order.
`ascent_levels()` (Kahn) yields levels with the leaves — components with no
component dependencies — at level 0, deterministic name order within a
level.

## 6. Version resolution — the cascade rule

`src/version_resolver.rs`. For each producer a consumer pins:

- **Not bumped this run** (a leaf, or a component whose own pins were
  already aligned): the target is the **latest pushed `main` tip**, queried
  read-only from the remote (ls-remote; queried once per component at run
  start).
- **Bumped this run**: the target is that producer's pushed
  **`synchronizer` branch tip**, taken from the run's `BumpLedger` — the
  monotonically growing map of components bumped so far to the tips this
  run pushed for them.

```text
resolve(producer) = SynchronizerTip(ledger[producer])   if producer in ledger
                    RemoteMainTip(ls-remote main)        otherwise
```

A pin is **stale** when the revision it currently locks differs from the
resolved target's revision. Staleness is computed per edge, so the two
layers of one producer bump independently but toward the same revision.

A producer whose verify **failed** stays in the ledger: consumers still pin
its pushed synchronizer tip — that tip is the aligned wire truth of this
run — and their own verifies surface the breakage as collected failures
(locked decision 7: broken synchronizer branches are accepted, not halted).

## 7. Actions — branch, commit, push

`src/git_repository.rs`. Per modified repository:

- The base of every bump commit is the repository's **remote `main` tip**
  (fetched into the configured clone's object store). Manifests are read at
  that tip too — never from the working copy, which may be mid-edit by an
  agent.
- The edits (up to four files: `Cargo.toml`, `Cargo.lock`, `flake.nix`,
  `flake.lock`) become **one commit** built at the object level — blobs,
  tree, commit — with no working copy created or modified. The commit
  message names the applied bumps.
- The commit is pushed to the tool-owned **`synchronizer` branch**,
  overwriting whatever a previous run left there: the branch is staging
  surface owned by this tool, rebuilt from `main`-tip truth each run, so a
  force update is the designed behavior (§14 q3). Nothing is ever pushed to
  any other ref.
- The plumbing behind this boundary (in-process `gix` vs shelling to git
  plumbing) is an implementation choice inside the stated invariants
  (§14 q4).

## 8. Verification — role-resolved builder

`src/role_resolution.rs` + `src/build_verify.rs`. After each push:

- The builder host is resolved from the configured CriomOS **role**
  through the `ClusterRoleDirectory` boundary:

  ```rust
  trait ClusterRoleDirectory {
      fn host_for(&self, role: &BuilderRole) -> Result<BuilderHost, Error>;
  }
  ```

  No hostname exists in the tool or its configuration; in the psyche's
  cluster the configured role currently resolves to prometheus, and that
  fact lives in the cluster configuration, not here. The concrete surface
  the production directory reads — the cluster flake's role outputs vs a
  Lojix query — is an implementation detail to confirm with OS-ops
  (§14 q5). Resolution happens once per run.
- The verify is the **narrow check**: `nix build` of the consumer's default
  package at the pushed revision, addressed as a remote flake reference
  (`github:<owner>/<component>/<revision>#default`) and executed on the
  resolved host (ssh invocation, per the push-first builder doctrine: the
  builder only sees pushed refs). Wide `nix flake check` sweeps are
  deliberately not the verify gate (they are loop-killers; kink ledger #24).
- A verification failure is report data, never a crate error.

## 9. Failure policy — collect, keep going

Only two conditions are run-fatal: an unreadable/undecodable configuration,
and undiscoverable topology (including a dependency cycle). Everything else
— a failed fetch, resolve, manifest edit, prefetch, commit, push, role
resolution, or verify — is recorded as a `Failure { component, stage,
detail }`, the affected repository's outcome reflects it, and the ascent
continues with every repository it can still process. All failures are
reported together at the end. If role resolution itself fails, bumps and
pushes still proceed and every verification reports `NotAttempted`.

## 10. Report (NOTA)

`src/report.rs`. One NOTA document per run, printed to stdout; the Rust
schema plus round-trip tests own the wire truth.

```nota
;; SynchronizerReport: one run's outcome, levels in ascent order (strict positional).
(SynchronizerReport <started-at> <finished-at> [<level-outcome>] [<failure>])
;;   started-at / finished-at: unix seconds

(LevelOutcome <index> [<repository-outcome>])

(RepositoryOutcome <component-name> <action> <verification>)
;;   action:
;;     AlreadyAligned — every pin already matched its resolved target
;;     (Bumped [<applied-bump>] (PushedBranch <branch-name> <tip-revision>))
;;     (BumpFailed <stage>) — detail in the failures vector
;;   verification:
;;     (Verified <builder-host>)
;;     (VerifyFailed <builder-host>) — detail in the failures vector
;;     NotAttempted

(AppliedBump <dependency-name> <pin-layer> <previous> <next>)
;;   pin-layer      : CargoManifest | CargoLock | FlakeManifest | FlakeLock
;;   previous / next: (Revision <commit>) | (Reference <branch-name>)

(Failure <component-name> <stage> <detail>)
;;   stage : Fetch | Resolve | ManifestEdit | LockEdit | Prefetch | Commit | Push | RoleResolution | Verify
;;   detail: pipe text — decode error or command output excerpt
```

This covers the four required contents: bumps applied (`AppliedBump`),
branches pushed (`PushedBranch`), per-level verify results
(`LevelOutcome`/`Verification`), and the collected failures (`[<failure>]`).

## 11. Algorithm — topological ascent

`src/driver.rs`, `SynchronizerRun::execute`:

```text
1  load configuration; open each component's GitRepository
2  query each remote main tip (read-only); fetch tips into the object stores
3  load ComponentManifests at each main tip
4  discover DependencyGraph; compute ascent_levels        (cycle => fatal)
5  ledger <- empty
   for each level, leaves first; for each component in the level:
     a  resolve each producer edge: ledger hit => SynchronizerTip,
        otherwise RemoteMainTip
     b  stale <- edges whose pinned revision != target revision
     c  if stale is empty: record AlreadyAligned / NotAttempted; continue
     d  apply typed bumps:
          CargoLock      — repin revision + package version at target
          CargoManifest  — redirect branch main -> synchronizer   (cascade pins only)
          FlakeLock      — set rev; prefetch narHash (the only nix call
                           in the pin path); set original.ref     (cascade pins only)
          FlakeManifest  — rewrite rev-in-URL                     (URL-pinned inputs only)
     e  commit the edited files on top of the remote main tip;
        push the tool-owned synchronizer branch (force)
     f  ledger.record(component, pushed tip)
     g  verify: nix build of the default package at the pushed revision
        on the role-resolved builder host
     h  every step's failure is collected; the ascent continues
6  render the SynchronizerReport as NOTA; exit nonzero when failures exist
```

The ascent is self-driving: nothing tells the tool what to bump. Staleness
against resolved targets is the whole trigger, and the ledger is what makes
level N+1 pin the synchronizer tips produced at level N.

## 12. Code map

```text
src/
├── lib.rs                 crate doc + module surface
├── main.rs                fn main only: argv -> run -> NOTA report -> exit code
├── error.rs               typed crate Error (thiserror); run-fatal + infrastructure only
├── types.rs               domain newtypes (ComponentName, CommitIdentifier, BranchName,
│                          BuilderRole, BuilderHost, NarHash, ...)
├── configuration.rs       NOTA config schema + load (canonical codec only)
├── cargo_manifest.rs      typed Cargo.toml model (serde/TOML) + cascade redirect
├── cargo_lock.rs          typed Cargo.lock model (serde/TOML) + git-pin repin
├── toml_pretty.rs         PrettyPrinter: canonical manifest + lock rendering
├── flake_lock.rs          typed flake.lock model (serde/JSON) + NarHashSource boundary
├── flake_manifest.rs      flake.nix input-URL scanner (winnow) + span rewrite
├── component_manifests.rs both pin surfaces of one component, read at its main tip
├── topology.rs            DependencyEdge/PinLayer, DAG discovery, ascent levels
├── version_resolver.rs    ResolvedTarget, BumpLedger, cascade rule, staleness
├── git_repository.rs      object-store git boundary: ls-remote, fetch, object-level
│                          commit, synchronizer-branch push
├── role_resolution.rs     ClusterRoleDirectory trait + CriomOS directory stub
├── build_verify.rs        BuildVerifier: default-package build at pushed rev on host
├── report.rs              NOTA report schema + rendering
└── driver.rs              SynchronizerRun: the ascent
tests/
├── configuration.rs       config round-trip + checkout resolution witnesses
├── report.rs              report round-trip witness
├── topology.rs            discovery-by-repository-identity + level/cycle witnesses
└── version_resolver.rs    cascade-rule witnesses
```

One capability per module; every behavior is a method on a data-bearing
type or a trait boundary; typed `Error` at the crate boundary.

## 13. Invariants

Test seeds — each of these should become a witness or review gate:

- The tool never pushes to, commits to, or edits `main` — locally or
  remotely — and only ever writes the `synchronizer` branch.
- No working copy, index, or current branch of any configured checkout is
  read or modified; manifests are read at fetched revisions from the object
  store.
- No hostname appears in source or configuration; builder selection passes
  through `ClusterRoleDirectory`.
- All manifest and lock manipulation is typed: serde for TOML and JSON,
  winnow for the git-source and input-URL grammars, the canonical NOTA
  codec (record qvb3) for config and report. The only text substitution is
  the flake.nix input-URL span rewrite of a parsed literal.
- The pin-editing path shells out only for `nix flake prefetch` (narHash);
  build execution exists only inside `BuildVerifier`; remote reads are
  ls-remote only.
- Per-repository failures are collected, never fatal; only configuration
  load and topology discovery abort a run.
- Topology edges derive only from manifests matched by repository identity
  (never package name, never configuration declarations).
- `SynchronizerConfig` and `SynchronizerReport` are strict positional NOTA:
  no optional in a positional slot; growth by trailing field or new
  variant.

## 14. Undecided — for psyche sign-off

Numbered questions referenced from the sections above. Everything outside
this list is proposed as accepted design.

1. **Cargo.toml comment loss.** Serde reserialization drops TOML comments;
   some component manifests (e.g. spirit's) carry load-bearing comments. A
   cascade bump must rewrite `Cargo.toml` (branch redirect), so those
   comments vanish *on the synchronizer branch*. `main` is never rewritten,
   but a wholesale merge of a synchronizer branch would land the stripped
   manifest. Accept (synchronizer branches are staging; integrators re-edit
   manifests when landing), or require comment-preserving TOML writes,
   which conflicts with the locked real-serde decision?
2. **Transitive lock completeness.** A typed `Cargo.lock` edit covers
   rev/version/branch of existing entries; it cannot add entries when a
   dependency's own dependency set changed at the target revision.
   Designed behavior: build-verify catches it, failure is collected. Should
   the tool additionally fall back to shelling `cargo update -p <package>`
   in exactly that case, or is verify-catches-it the accepted stop line?
3. **Synchronizer branch lifecycle.** Designed: the branch is tool-owned
   staging, rebuilt each run as one commit on the current remote `main` tip
   and force-pushed; merge-back to `main` is out of scope. Confirm.
4. **Git plumbing choice.** The `GitRepository` boundary and its invariants
   are the design; behind it, in-process `gix` vs shelling to git plumbing
   (the raw-git workspace boundary governs agent work — does it also bind
   this tool's internals, which would suggest jj or gix?). Preference?
5. **Cluster-config surface** (OS-ops confirm). Which concrete surface
   should `CriomosClusterDirectory` read for role→host — the cluster
   flake's node/role outputs (CriomOS-test-cluster shape), a Lojix query,
   or another artifact? Is `(ClusterFlake <flake-reference>)` the right
   configuration carrier for it?
6. **Verify target.** Designed: default package build only (narrow check).
   Should any component be verified with a named check set instead, and if
   so, should that live in configuration or stay out of scope?
7. **Third-party input scope.** Component pins only, by design. Should the
   same machinery later bump shared third-party inputs (fenix — the kink #1
   toolchain-pin rot, nixpkgs, crane) toward a configured target, as a
   second phase (relates to primary-95fm)? Out of scope for this
   implementation unless directed.
