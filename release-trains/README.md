# Release-train intents

Each `release-trains/<name>.nota` is an authored **intent**, not a lockfile.
It names component selectors and their expected bases; Synchronizer resolves it
against pushed remote truth, discovers dependencies from those selected
manifests, and emits an immutable closure before testing.

`language-family-poc.nota` is the seed for the NOTA → schema-language →
schema-rust train. NOTA is fixed to the pushed green `18e2e8d0dba37e9e84045af3608585b51f6e3b36` candidate. The zero bases for not-yet-created Schema
branches are deliberate placeholders: resolution must reject them until their
pushed branch tips and expected bases are recorded.

A resolved train has these projections:

```text
intent NOTA → exact commit closure → candidate Cargo.lock/flake.lock
            → release-train.lock.json → fixed-input integration flake
```

Do not replace a component's Cargo.lock with a global lock. Cargo resolves
one lock per manifest/workspace. Do not put local paths into generated train
artifacts. A candidate branch has the form `train/<name>` and is reproducible
only through its resolved commit identity.
