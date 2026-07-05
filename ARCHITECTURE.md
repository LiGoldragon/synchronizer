# synchronizer — architecture

*Meta-repo version propagation: cascade dependency bumps across the
component tree so wire contracts stay aligned.*

This document is the psyche-signed design the implementation follows. The
formerly open questions (§14 of the sign-off draft) are resolved and folded
into their owning sections; two of those resolutions are psyche-locked and
called out as such where they land (format-preserving manifest writes, §4;
wire-exercising verify, §8).

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

## 0a. The universality law (psyche-signed)

**Only genuinely deterministic behavior lives in code. Everything that
varies by project is configuration.** The tool carries ZERO project data:
no criome hostname, repository name, path, account, branch name, role name,
commit identity, or check-name convention appears anywhere in `src/`. Anyone
runs it against their own repositories purely by writing their own typed
NOTA config — no code change, no criome assumption baked in.

What this law puts behind config (§3):

- **Forge** — an abstraction (`Forge` enum with a method surface); GitHub is
  the sole current implementation, and all forge-shaped URL/flake-reference
  construction is centralized behind it. A closed enum is the workspace-
  correct form of this interface: the variant set lives in the type system
  and decodes from NOTA (a trait object cannot). New forges join by variant.
- **Branch scheme** — the mainline branch (the default version target) and
  the staging branch (the tool-owned branch every bump is pushed to) are
  both configured. Nothing assumes `main` or `synchronizer`.
- **Builder-host resolution** — a generic strategy interface
  (`BuilderResolution`): name the host directly (`DirectHost`), or resolve a
  role through a cluster directory (`ClusterRole`). The CriomOS
  cluster-datom resolver is **one optional plugin**, never the only path and
  never hard-coded.
- **Verify-gate selection** — the wire-exercising check-name words are
  configured (`VerifyPolicy::WireExercising`), or a project chooses
  `DefaultBuild` and every repo is verified by the default `nix build`.
- **Commit author** — the author/committer name and email are configured.
- **Topology / edge identity** — repository identity comes from the git URL
  (deterministic parsing) matched against the configured component set and
  forge owner; edges are never declared.

What this law keeps in code, because it is deterministic, not project-varying:

- **The cascade rule** — bumped-this-run targets the staging tip, everything
  else targets the mainline tip. Only the *branch names* vary (config); the
  rule is the algorithm.
- **The flake-input `original`-preservation rule** — Nix re-resolves an
  input's `original` from `flake.nix` on update and discards a lock whose
  `original` mismatches the declaration, re-locking to the declared branch
  tip. Preserving the `original` and carrying the cascade in `locked.rev`
  alone is therefore forced by Nix's own lock/declaration reconciliation
  (empirically proven against Nix 2.34.6, git- and github-type inputs;
  `tests/nix_resolution.rs`). It is uniform across every Nix-flake project,
  so making it configurable would only let a consumer choose a broken
  behavior. It stays in code (§4).

## 0b. Locked design decisions

The following design decisions are psyche-decided and locked. Implementation
refines them; it does not relitigate them:

1. configuration is NOTA;
2. both pin layers are managed — Cargo (Cargo.toml + Cargo.lock) and flake
   (flake.lock, plus flake.nix where the pin lives in the URL) — as typed
   data, never string munging;
3. topology is discovered from the manifests, not declared in configuration;
4. version resolution is cascade-aware: unchanged dependencies target the
   latest pushed mainline tip, dependencies bumped this run target their
   pushed staging-branch tip;
5. every modified repo gets a commit pushed to a dedicated staging branch —
   never the mainline;
6. every bump is build-verified on a builder whose host is resolved through
   the configured builder-resolution strategy — no hostname anywhere in the
   tool;
7. on failure the run keeps going, collects all failures, and reports them
   together; pushed-but-broken staging branches are accepted;
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

The configuration carries every project-specific fact (§0a): the participants
and where their clones live, the branch scheme, the builder-host strategy,
the verify policy, and the commit author. It never declares dependency edges
(§5). Decoding goes only through the canonical NOTA codec (Spirit record
qvb3); the Rust schema in `src/configuration.rs` plus round-trip tests own
the wire truth — the pseudo-NOTA here is documentation. The record names
below label the schema; on the wire the root is an untagged positional struct
record, per the codec's untagged-struct rule. A multi-field enum variant is a
tag plus one grouped payload record: `(ClusterRole (<role> <source>))`.

```nota
;; SynchronizerConfig: root configuration document (strict positional, untagged).
(<forge>
 <checkout-root>
 [<component>]
 <branch-scheme>
 <builder-resolution>
 <verify-policy>
 <commit-author>)
;;   forge             : (GitHub <owner>) — forge account holding every component remote
;;   checkout-root     : absolute path holding local clones named after components
;;   component         : (<name> <checkout>) — untagged Component record
;;   branch-scheme     : (<mainline> <staging>) — version-target branch / tool-owned branch
;;   builder-resolution: how the build-verify host is determined (§8)
;;   verify-policy     : how the verify gate is selected (§8)
;;   commit-author     : (<name> <email>) — author/committer of every bump commit

;; Component: one participating repository (untagged).
(<name> <checkout>)
;;   name    : bare atom — the repository name, e.g. signal-router
;;   checkout: AtRoot | (AtPath <absolute-path>)
;;     AtRoot   — the clone lives at <checkout-root>/<name> (ghq-style layout)
;;     (AtPath <absolute-path>) — explicit override

;; BranchScheme: the branch names a run works against (untagged, 2 fields).
(<mainline> <staging>)
;;   mainline: bare atom — the default version-target branch, e.g. main / master / trunk
;;   staging : bare atom — the tool-owned branch every bump is pushed to (force), never mainline

;; BuilderResolution: how the build-verify host is found (closed enum).
(DirectHost <builder-host>)
;;   a literal host — no cluster, no role indirection
(ClusterRole (<builder-role> <cluster-source>))
;;   resolve <builder-role> to a host through <cluster-source>
;;     builder-role : bare atom — the cluster role of the host, e.g. NixBuilder
;;     cluster-source: (ClusterProposal <absolute-path>) — the horizon-rs ClusterProposal datom (§8)

;; VerifyPolicy: how the verify gate is selected (closed enum).
(WireExercising [<wire-word>])
;;   enumerate checks; build those whose hyphen/underscore-split name carries a listed
;;   word (e.g. daemon, socket, wire); fall back to the default package build where none match
DefaultBuild
;;   always the default `nix build` of the flake; never enumerate checks

;; CommitAuthor: the identity stamped on every bump commit (untagged, 2 fields).
(<name> <email>)
;;   name : bare atom — e.g. synchronizer
;;   email: bare atom — e.g. ci@example.org
```

Example (the criome instance lives externally at
`goldragon/synchronizer.nota`, §3a):

```nota
((GitHub LiGoldragon)
 /git/github.com/LiGoldragon
 [(signal-frame AtRoot)
  (signal-router AtRoot)
  (signal-harness AtRoot)
  (introspect AtRoot)]
 (main synchronizer)
 (ClusterRole (NixBuilder (ClusterProposal /git/github.com/LiGoldragon/goldragon/datom.nota)))
 (WireExercising [daemon daemons socket sockets wire])
 (synchronizer synchronizer@criome.net))
```

A fully non-criome instance — a different forge account, a `master`/`bump-train`
scheme, a directly named host, and the default-build gate — decodes through
the same schema (witnessed in `tests/configuration.rs`):

```nota
((GitHub octocat)
 /home/dev/src
 [(alpha AtRoot) (beta (AtPath /home/dev/checkouts/beta))]
 (master bump-train)
 (DirectHost buildbox.local)
 DefaultBuild
 (ci-bot ci@octocat.example))
```

Notes:

- Field order is the positional wire contract; growth happens by trailing
  field or new variant, never by reordering.
- `checkout` is a closed enum with a required payload on the override
  variant — no optional in a positional slot.
- Neither branch name is a tool constant: both are the `BranchScheme` field.
  The tool holds no `main`/`synchronizer` literal (§0a).
- `(ClusterProposal <absolute-path>)` names the confirmed CriomOS cluster
  surface: the horizon-rs `ClusterProposal` datom (§8). It is one optional
  cluster strategy; growth to other surfaces happens by new `ClusterSource`
  variant, and a project needing no cluster at all uses `DirectHost`.

## 3a. Where the criome configuration lives

The tool ships no criome data. The criome instance lives in the public
criome infra-data repository `LiGoldragon/goldragon` as `synchronizer.nota`,
beside the `datom.nota` cluster proposal its `ClusterRole` strategy reads —
so the config and the cluster surface it depends on version together in one
jj-managed, pushed repository. goldragon is data-only (no code, no flake),
which is exactly the right home for a typed-NOTA config file; primary
`reports/` and `beads/` are deliberately not used. A consumer validates a
config edit without a live run via `cargo run --example validate --
<config.nota>` (decode only; no git, nix, or network).

## 4. Pin layers

A dependency between two components is pinned in up to four places, and
each is a typed model:

### Cargo layer

- **Format-preserving writes (psyche-locked).** Every TOML write goes
  through a typed, format-preserving document
  (`src/toml_document.rs`, `toml_edit::DocumentMut`): a bump changes
  exactly the edited values and leaves every comment, alignment, and layout
  byte untouched (spirit's `Cargo.toml` carries load-bearing comments).
  Serde models remain the read surface for topology and staleness. Real
  typed data, non-destructive — never string munging.
- **`Cargo.toml`** (`src/cargo_manifest.rs`) — serde through the TOML
  deserializer for reading; the format-preserving document for writing.
  In this workspace sibling dependencies are declared
  `{ git = "https://github.com/LiGoldragon/<repo>.git", branch = "main" }`,
  so the manifest changes **only** on a cascade pin: the dependency is
  redirected `branch = "main"` → `branch = "synchronizer"` so Cargo can
  reach the locked revision from a fresh clone (a rev locked off-branch is
  unreachable through a `branch = "main"` declaration).
- **`Cargo.lock`** (`src/cargo_lock.rs`) — serde-typed reading with a
  winnow grammar over the `git+<url>?<query>#<rev>` source shape — never ad
  hoc splitting. A bump sets the locked revision, rewrites the reference
  query to match the manifest, and synchronizes the recorded package
  `version` to what the dependency's own manifest declares at the target
  revision (read through the dependency's object store, §7). The write is
  the format-preserving document edit; Cargo's own rendering, `@generated`
  header included, survives byte-for-byte outside the edited entries.
- **Package name vs repository name vs table key.** A Cargo dependency
  resolves to a package name that may differ from the repository name
  (`nota` lives in `codec-repository`) *and* from the dependency table key
  (`codec-repository = { package = "nota", ... }`). Component matching goes
  through the git URL's repository identity, never the package name; the
  format-preserving document is addressed by the table key
  (`DependencyKey`), never the resolved package name.
- **Multi-pin and rev-pin safety.** A dependency deliberately pinned by
  `rev =`/`?rev=` or tag is **unbumpable**: the bump fails loud
  (`Error::UnbumpablePin`, a collected failure) instead of emitting an
  invalid `branch` + `rev` manifest. sema-engine — which spirit pins at
  deliberate old revisions — therefore stays out of the first configured
  set. A producer declared under **several same-name manifest entries**
  (the same crate in `[dependencies]` and `[dev-dependencies]`, or two keys
  sharing a `package =` rename) is *not* unbumpable: every such entry
  follows the one producer, so topology collapses them to a single edge and
  the manifest edit redirects **every** textual entry coherently, each
  addressed by its own table key. A `Cargo.lock` that records the same
  package under several same-name git entries at genuinely different
  revisions *is* unbumpable — no single target rev repins them coherently —
  and fails loud there.
- **Transitive gaps** (`src/transitive_lock.rs`) — a typed lock edit cannot
  invent new transitive entries when the dependency's own dependency set
  changed at the target revision. The accepted controlled fallback for
  exactly that gap is `cargo update -p <package> --precise <revision>`,
  run in a scratch materialization of the consumer's base tree (no working
  copy is touched); the refreshed lock becomes the commit content. Gap
  detection compares the producer's declared dependency names at the target
  against the lock's recorded set (and triggers whenever the producer's
  published version is unknowable from its root manifest). Build-verify
  remains the final word on whatever the fallback cannot fix.

### Flake layer

- **`flake.lock`** (`src/flake_lock.rs`) — typed JSON via serde
  (`FlakeLock`, `LockNode`, `LockedSource`, `OriginalSource`; unknown
  fields preserved through typed remainders). A bump sets `locked.rev`
  in-type and obtains `narHash` and `lastModified` through the
  `NarHashSource` boundary — `nix flake prefetch --json`, the **only**
  external command in the whole pin-editing path, because the narHash is
  the one value text manipulation cannot produce. The node's `original`
  is **always preserved**, on cascade pins too: the locked `rev` alone
  carries the cascade. Editing `original.ref` to follow the synchronizer
  branch is wrong and was retired — Nix re-resolves originals from
  `flake.nix` on update, so a lock whose `original` mismatches what
  `flake.nix` declares is discarded and the input re-locked from the
  declaration (back to the `main` tip), evaluating the *old* content and
  silently reintroducing the exact skew this tool exists to kill
  (empirically proven against Nix 2.34.6 for git- and github-type
  inputs; the github-type witness is `tests/nix_resolution.rs`).
  Reserialization is Nix's canonical lock rendering (two-space indent,
  sorted keys).
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
each bumped and reported independently. A producer named under several
same-name entries of one layer (the same crate in `[dependencies]` and
`[dev-dependencies]`) yields one edge per entry during discovery; those are
collapsed so the invariant holds — one edge per (consumer, producer, layer,
local name), the layer bumped once, the manifest edit redirecting every
textual entry behind that edge. Anything pointing outside the configured set
produces no edge. The result must be a DAG; a cycle is
run-fatal (`Error::DependencyCycle`) because it admits no ascent order.
`ascent_levels()` (Kahn) yields levels with the leaves — components with no
component dependencies — at level 0, deterministic name order within a
level.

## 6. Version resolution — the cascade rule

`src/version_resolver.rs`. The mainline and staging branch names come from
the configured `BranchScheme` (§3); the resolver holds the scheme. For each
producer a consumer pins:

- **Not bumped this run** (a leaf, or a component whose own pins were
  already aligned): the target is the **latest pushed mainline tip**, queried
  read-only from the remote (ls-remote of the configured mainline branch;
  queried once per component at run start).
- **Bumped this run**: the target is that producer's pushed **staging-branch
  tip**, taken from the run's `BumpLedger` — the monotonically growing map of
  components bumped so far to the tips this run pushed for them.

```text
resolve(producer) = SynchronizerTip(ledger[producer])       if producer in ledger
                    RemoteMainTip(ls-remote <mainline>)      otherwise
```

The cascade *rule* is deterministic and lives in code (§0a); only the branch
names it names are configuration.

A pin is **stale** when the revision it currently locks differs from the
resolved target's revision. Staleness is computed per edge, so the two
layers of one producer bump independently but toward the same revision.

A producer whose verify **failed** stays in the ledger: consumers still pin
its pushed synchronizer tip — that tip is the aligned wire truth of this
run — and their own verifies surface the breakage as collected failures
(locked decision 7: broken synchronizer branches are accepted, not halted).

## 7. Actions — branch, commit, push

`src/git_repository.rs`. Per modified repository:

- The base of every bump commit is the repository's **remote mainline tip**
  (fetched into the configured clone's object store). Manifests are read at
  that tip too — never from the working copy, which may be mid-edit by an
  agent.
- The edits (up to four files: `Cargo.toml`, `Cargo.lock`, `flake.nix`,
  `flake.lock`) become **one commit** built at the object level — blobs,
  tree, commit — with no working copy created or modified. The commit
  message names the applied bumps, and the commit's author and committer are
  the configured `CommitAuthor` (§3) — the tool holds no author identity of
  its own.
- The commit is pushed to the tool-owned **staging branch** (the
  configured `BranchScheme` staging name), overwriting whatever a previous
  run left there: the branch is staging surface owned by this tool, rebuilt
  from mainline-tip truth each run, so a force update is the designed
  behavior. Merge-back to the mainline is out of scope. Nothing is ever
  pushed to any other ref.
- Object operations are in-process **gix** (accepted default: typed git
  library; blob/tree/commit built with no working copy). Transport —
  ls-remote, fetch, force-push of the synchronizer branch — is a typed
  invocation of git plumbing behind the same boundary, because gix 0.85
  implements no push; fetched tips land in the neutral
  `refs/synchronizer/*` namespace so no branch, remote-tracking ref, index,
  or working copy moves.

## 8. Verification — configured builder-host strategy

`src/driver.rs` + `src/role_resolution.rs` + `src/build_verify.rs`. After
each push:

- The builder host is resolved once per run through the configured
  `BuilderResolution` strategy (`src/driver.rs::ConfiguredBuilderHost`):

  ```rust
  enum BuilderResolution {
      DirectHost(BuilderHost),               // a literal host, no cluster
      ClusterRole(BuilderRole, ClusterSource) // a role through a cluster directory
  }
  ```

  No hostname exists in the tool. `DirectHost` returns the configured host
  as-is — the path for a consumer with no cluster directory. `ClusterRole`
  resolves through a `ClusterRoleDirectory` boundary
  (`fn host_for(&self, role) -> Result<BuilderHost, Error>`); the CriomOS
  cluster-datom resolver is one optional implementation of that boundary,
  never the only path and never hard-coded.
- **The CriomOS cluster strategy** (`CriomosClusterDirectory`) reads the
  cluster proposal document — the horizon-rs `ClusterProposal` NOTA datom
  (production: `goldragon/datom.nota`) whose per-node `services` vectors
  author every cluster role, e.g. `(NixBuilder (Some 6))`. Cluster flakes
  expose no role→host output, the production cluster repository is not a
  flake, and Lojix records deployment generations, not roles. The directory
  decodes a narrow, count-strict positional view of that datom (root 5
  fields, node 17 fields) through the canonical codec primitives and selects
  among the online nodes holding the role by declared capacity (absent
  meaning one job), name order breaking ties; a schema-count mismatch fails
  loud as a collected RoleResolution failure rather than misreading
  positions. The service capacity is read generically from the queried
  role's trailing payload — no service-kind name is hard-coded. In the
  psyche's cluster `NixBuilder` currently resolves to prometheus, a fact that
  lives in the datom, not here.
- **The verify is wire-exercising (psyche-locked), and its words are
  configured.** Where the repository's flake exposes checks that build *and
  launch* the daemons — the class that caught the build-vintage skew at
  runtime — those checks are the gate: the builder enumerates
  `checks.<system>` at the pushed revision and builds the wire-exercising
  class, selected by the check-name words the configured
  `VerifyPolicy::WireExercising` carries (the criome instance uses
  `daemon`/`socket`/`wire` and plurals; `src/build_verify.rs::WireCheckClassifier`).
  Which words mark the class is a per-project naming convention, so they are
  configuration, not a tool constant. Only where no such check exists does
  the verify fall back to the default `nix build` of the flake; a project
  with no such convention configures `VerifyPolicy::DefaultBuild` and skips
  enumeration entirely. A green plain build alone is not sufficient evidence.
  Everything is addressed as a remote flake reference
  (`github:<owner>/<component>/<revision>`) and executed on the resolved
  host over ssh, per the push-first builder doctrine: the builder only sees
  pushed refs. Wide `nix flake check` sweeps remain deliberately out (they
  are loop-killers; kink ledger #24).
- **Absence is data; failure is failure.** The check enumeration
  (`CheckEnumeration`) answers an absent `checks.<system>` attribute as an
  empty list inside the eval expression itself, so the default-build
  fallback happens only for genuine absence. An ssh or eval failure while
  enumerating is a collected verify failure — it never silently downgrades
  the gate to a plain build. The report records which gate class passed
  (`Verification::Verified` carries `WireChecks | DefaultPackage`), so a
  downgraded verify stays visible.
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

A cycle is diagnosed only among the graph's own members. A configured
producer whose fetch or load failed is **not** a cycle: its consumers are
still placed in the ascent and their unresolvable edges are collected as
Resolve failures (`Error::ProducerUnavailable`), keeping the run going.

## 10. Report (NOTA)

`src/report.rs`. One NOTA document per run, printed to stdout; the Rust
schema plus round-trip tests own the wire truth. As in §3, the record names
label the schema; struct records are untagged on the wire.

```nota
;; SynchronizerReport: one run's outcome, levels in ascent order (strict positional, untagged).
(<started-at> <finished-at> [<level-outcome>] [<failure>])
;;   started-at / finished-at: unix seconds

;; LevelOutcome (untagged)
(<index> [<repository-outcome>])

;; RepositoryOutcome (untagged)
(<component-name> <action> <verification>)
;;   action:
;;     AlreadyAligned — every pin already matched its resolved target
;;     (Bumped [<applied-bump>] (PushedBranch <branch-name> <tip-revision>))
;;     (BumpFailed <stage>) — detail in the failures vector
;;   verification:
;;     (Verified (<builder-host> <verification-gate>))
;;     (VerifyFailed <builder-host>) — detail in the failures vector
;;     NotAttempted
;;   verification-gate: WireChecks | DefaultPackage — which verify class
;;     passed, so a default-build downgrade stays visible in the report

;; AppliedBump (untagged)
(<dependency-name> <pin-layer> <previous> <next>)
;;   pin-layer      : CargoManifest | CargoLock | FlakeManifest | FlakeLock
;;   previous / next: (Revision <commit>) | (Reference <branch-name>)

;; Failure (untagged)
(<component-name> <stage> <detail>)
;;   stage : Fetch | Resolve | ManifestEdit | LockEdit | Prefetch | Commit | Push | RoleResolution | Verify
;;   detail: string — decode error or command output excerpt
;;   run-scoped failures (role resolution) carry the component name synchronizer
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
     d  apply typed bumps (format-preserving writes):
          CargoLock      — repin revision + package version at target;
                           transitive gap => cargo update --precise fallback
          CargoManifest  — redirect branch mainline -> staging   (cascade pins only;
                           every same-name entry of the producer redirected)
          FlakeLock      — set rev; prefetch narHash (the only nix call
                           in the pin path); original always preserved
          FlakeManifest  — rewrite rev-in-URL                    (URL-pinned inputs only)
          unbumpable pin (deliberate rev/tag pin, or a lock's same-name
                           multi-version aliasing)
                         => fail loud, collected, pin left alone
     e  commit the edited files on top of the remote mainline tip
        (author = configured CommitAuthor);
        push the tool-owned staging branch (force)
     f  ledger.record(component, pushed tip)
     g  verify at the pushed revision on the configured builder host:
        the repo's wire-exercising checks (configured words), or the
        default nix build where it has none / when DefaultBuild is set
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
│                          BuilderRole, BuilderHost, AuthorName, AuthorEmail, ...) +
│                          repository-identity grammar
├── configuration.rs       NOTA config schema (Forge, BranchScheme, BuilderResolution,
│                          ClusterSource, CommitAuthor) + load (canonical codec only)
├── toml_document.rs       PreservedTomlDocument: the format-preserving TOML write surface
├── cargo_manifest.rs      typed Cargo.toml model (serde read + preserved write) + cascade redirect
├── cargo_lock.rs          typed Cargo.lock model (serde read + preserved write) + git-pin repin
├── transitive_lock.rs     TransitiveLockResolver boundary + cargo update --precise fallback
├── flake_lock.rs          typed flake.lock model (serde/JSON) + NarHashSource boundary
├── flake_manifest.rs      flake.nix input-URL scanner (winnow) + span rewrite
├── component_manifests.rs both pin surfaces of one component, read at its main tip
├── topology.rs            DependencyEdge/PinLayer/LocalPinName, DAG discovery, ascent levels
├── version_resolver.rs    ResolvedTarget, BumpLedger, cascade rule, staleness
├── git_repository.rs      ComponentRepository boundary + gix object store: ls-remote, fetch,
│                          object-level commit, synchronizer-branch push
├── role_resolution.rs     ClusterRoleDirectory trait + CriomOS cluster-proposal directory
│                          (one optional builder-host strategy)
├── build_verify.rs        Verifier boundary + BuildVerifier: VerifyPolicy-driven checks at
│                          pushed rev on the resolved host; WireCheckClassifier
├── report.rs              NOTA report schema + rendering
└── driver.rs              SynchronizerRun: boundary composition + the ascent;
                           BuilderHostResolver + ConfiguredBuilderHost strategy dispatch
examples/
└── validate.rs           decode-only config validator (no git/nix/network)
tests/
├── fixtures/mod.rs        shared in-memory boundaries (repository, prefetch, verifier)
├── build_verify.rs        wire-check classification + installable addressing witnesses
├── cargo_lock.rs          pin-grammar + format-preserving repin witnesses
├── cargo_manifest.rs      comment/layout-preservation witnesses
├── configuration.rs       config round-trip + checkout resolution witnesses
├── driver.rs              the three-component fixture ascent (cascade end to end)
├── flake_lock.rs          byte round-trip + rev-set fidelity witnesses
├── flake_manifest.rs      scanner + span-rewrite witnesses
├── git_repository.rs      gix object-level commit witness (no ref moves)
├── nix_resolution.rs      stateful builder probe (ignored): real Nix resolves a
│                          cascaded lock to the pinned rev (Preserve semantics)
├── report.rs              report round-trip witness
├── role_resolution.rs     cluster-proposal role→host witnesses
├── topology.rs            discovery-by-repository-identity + level/cycle witnesses
└── version_resolver.rs    cascade-rule + per-layer staleness witnesses
```

One capability per module; every behavior is a method on a data-bearing
type or a trait boundary; typed `Error` at the crate boundary.

## 13. Invariants

Test seeds — each of these should become a witness or review gate:

- The tool carries zero project data: no hostname, repository name, path,
  account, branch name, role name, commit identity, or check-name convention
  appears in `src/`. Every such fact is a configuration field (§0a); a
  non-criome config exercises the same generic paths
  (`tests/configuration.rs`, `tests/driver.rs::generic`).
- The tool never pushes to, commits to, or edits the mainline branch —
  locally or remotely — and only ever writes the configured staging branch.
  Neither branch name is a tool literal.
- No working copy, index, or current branch of any configured checkout is
  read or modified; manifests are read at fetched revisions from the object
  store.
- No hostname appears in source; builder selection passes through the
  configured `BuilderResolution` strategy (`DirectHost`, or `ClusterRole`
  through a `ClusterRoleDirectory` — the CriomOS resolver being one optional
  plugin, never the only path).
- All manifest and lock manipulation is typed: serde for TOML and JSON
  reading, the format-preserving TOML document for TOML writing, winnow for
  the git-source and input-URL grammars, the canonical NOTA codec (record
  qvb3) for config and report. The only text substitution is the flake.nix
  input-URL span rewrite of a parsed literal.
- A pin write is non-destructive: comments, alignment, and layout of
  everything untouched survive byte-for-byte.
- A flake-lock repin never edits a node's `original`: the locked rev alone
  carries the cascade, because Nix discards a lock whose original
  mismatches `flake.nix` and re-locks from the declaration.
- A verify never silently downgrades: check-enumeration failure is a
  collected failure (only genuine absence falls back to the default
  build), and a passed verification names its gate class in the report.
- A deliberately rev- or tag-pinned dependency is never bumped
  mechanically: the bump fails loud as a collected failure and the pin is
  left alone. A producer under several same-name *manifest* entries is
  bumped — every entry is redirected coherently — but a *lock* recording
  the same package under several same-name git entries at different
  revisions is not (no single target rev repins them).
- The pin-editing path shells out only for `nix flake prefetch` (narHash)
  and, on a detected transitive gap, the controlled
  `cargo update --precise` fallback in a scratch tree; build execution
  exists only inside `BuildVerifier`; git transport (ls-remote, fetch,
  staging-branch push) exists only inside `GitRepository`.
- Per-repository failures are collected, never fatal; only configuration
  load and topology discovery abort a run.
- Topology edges derive only from manifests matched by repository identity
  (never package name, never configuration declarations).
- The configuration and report roots are strict positional NOTA, untagged
  struct records: no optional in a positional slot; growth by trailing
  field or new variant.

## 14. Resolved sign-off decisions

The sign-off draft's open questions, as decided by the psyche and folded
into the sections above:

1. **Cargo.toml comments are preserved** (locked): format-preserving
   toml_edit writes; serde stays the read model (§4).
2. **Transitive lock gaps** get the controlled
   `cargo update -p <package> --precise <revision>` fallback (§4).
3. **Synchronizer branch lifecycle** confirmed: tool-owned staging,
   force-rebuilt each run from the remote `main` tip; merge-back out of
   scope (§7).
4. **Git plumbing**: gix for object operations; typed git plumbing for
   transport, gix having no push (§7).
5. **Cluster surface**: the cluster proposal datom
   (`(ClusterProposal <absolute-path>)`), confirmed by OS-ops discovery
   (§8).
6. **Verify target** (locked): the wire-exercising check class, default
   build only where a repo has none (§8).
7. **Third-party inputs stay out of scope**: only configured component
   pins cascade; fenix/nixpkgs/crane convergence (primary-95fm) is separate
   work (§1).
8. **Universality refactor** (§0a): every project-specific fact moved behind
   typed config — branch scheme, builder-host strategy (`DirectHost` |
   `ClusterRole`, the CriomOS resolver demoted to one optional plugin),
   verify-policy words, and commit author. The tool source carries zero
   criome data; the criome instance lives at `goldragon/synchronizer.nota`
   (§3a). The cascade rule and the flake-input `original`-preservation rule
   stay in code because they are deterministic, not project-varying.
