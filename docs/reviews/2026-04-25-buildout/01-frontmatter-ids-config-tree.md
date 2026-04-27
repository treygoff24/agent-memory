# Clean-code review — frontmatter / ids / config / tree

Reviewer: reviewer-frontmatter
Date: 2026-04-25
Spec anchor: `docs/specs/stream-a-core-substrate-v1.1.md` (v1.1)

Files reviewed:

- `crates/memory-substrate/src/frontmatter/{mod,parse,validate,schema,defaults,serialize}.rs`
- `crates/memory-substrate/src/ids/{mod,sequence,repair}.rs`
- `crates/memory-substrate/src/config/mod.rs`
- `crates/memory-substrate/src/tree/{mod,validate,layout}.rs`

The review is static — `cargo` was not invoked. Some findings cite `model.rs` / `api.rs` / `git/adopt.rs` because those files house behavior that the in-slice modules either rely on or are meant to enforce.

---

## Blockers

### B1. `repair_duplicate_ids` does not implement spec §7.3 — multiple violations in one function

`crates/memory-substrate/src/ids/repair.rs:13-49`

The current `repair_duplicate_ids` implementation diverges from spec §7.3 in five separate, independently failing ways. Codex's "release-certification candidate" claim cannot survive any one of these.

1. **Survivor selection is wrong (spec §7.3.2).** Spec mandates: "Select canonical survivor by earliest `(created_at, git commit timestamp, device_id, path)`." The code (`repair.rs:18-25`) keeps whichever record `relative_memory_paths` returned first and renames every later occurrence. Walk order from `walkdir` is not stable across filesystems — two clones of the same repo can pick different survivors. **Fix:** load all duplicates into a candidate set, sort by `(created_at, commit timestamp via `git log -1 --format=%ct`, device_id, path)`, keep index 0.
2. **References are not rewritten (spec §7.3.5).** The spec requires rewriting `supersedes`, `superseded_by`, `related`, and evidence ID refs in every file that points at a renamed memory. The code only renames the duplicate file's own filename and frontmatter `id` (`repair.rs:32-44`). After repair, every other file that referenced the renamed ID is now silently broken. The validator will then either flag `MissingReference` (if `FullySynced`) or warn (if `PartialSync`), but the repair function returns success. **Fix:** after computing all renames, iterate every memory file in the repo, rewrite ID arrays/strings against the rename map, and re-serialize via `serialize_document`.
3. **Sequence allocator is bypassed (spec §7.3.3, §7.3.6).** Spec: "mint a new valid ID with `ids::mint_next_unused(date, local_shard, reserved_ids)`. The allocator advances `seq.json` past every existing ID for `(date, local_shard)` before returning." The code uses a private `next_unused_like` (`repair.rs:73-88`) that picks `old_id.sequence + 1` without ever touching `seq.json`. Acceptance signal §7.3.6 ("Duplicate repair where local `seq.json.next` lags existing same-shard IDs mints the next unused repo-visible ID, not another duplicate") will fail: a local device that has minted no IDs today will start at `seq=1` even after repair stuffed the repo with that shard's `000087` and `000088`. **Fix:** call `next_memory_ids(runtime, device_id, &reserved, count)` from `sequence.rs`. Pass in the full repo-visible ID set as `reserved`.
4. **Cross-shard corruption (spec §7.3 plus §10/§7.1 invariants).** `next_unused_like` keeps `prefix` as `mem_<date>_<old_shard>` (`repair.rs:74-82`). The motivating duplicate scenario per spec §7.3 is "a repo cloned/copied without adoption" — which means duplicates very likely live in another device's shard. After repair the _local_ device just minted IDs in _another device's shard namespace_. That breaks the device-shard identity invariant. **Fix:** mint the new IDs in the _local_ device's shard via the standard allocator (per #3 above). Filenames change from `mem_<date>_<their_shard>_<seq>.md` to `mem_<date>_<local_shard>_<new_seq>.md`.
5. **No event emission, no reindex (spec §7.3.6, §7.3.7).** Spec requires `DuplicateIdRepaired` events and reindex of affected files. Neither happens. The audit trail for repair is silently lost.

This is one function and one bug surface, but the spec lists six requirements and only two-and-a-half are implemented. Treat the whole function as needing rewrite, not patch.

### B2. `TreeValidationMode::StartupPreflight` is defined but not actually distinct from `FullySynced`

`crates/memory-substrate/src/tree/validate.rs:13-72`

Spec §5.4 is explicit: in `StartupPreflight` mode the tree validator must additionally check "local git merge-driver config presence." `validate_tree` only branches on `PartialSync` vs everything else (`validate.rs:63-68, 144-149`); `StartupPreflight` collapses into `FullySynced` semantics with zero merge-driver inspection. Acceptance signal §5.5 ("Fresh clone plus `git::adopt_clone` regenerates ... merge-driver config") therefore can't be verified by tree validation alone. **Fix:** when `mode == StartupPreflight`, run `git config --get merge.memory-merge-driver.driver` (or call into `git::preflight`) and surface a typed error if absent. Today the variant is dead code; that's worse than spec drift, because it implies coverage that doesn't exist.

### B3. Tree validator skips most spec §5.4 checks — coverage is misleading

`crates/memory-substrate/src/tree/validate.rs:32-71`

Spec §5.4 lists eight responsibilities. The current implementation covers four: case-fold collisions, duplicate IDs, ID-filename match, supersession graph (acyclicity + inverse). Missing:

- **Canonical path patterns / slug / date validity** — neither slug regex `[a-z0-9][a-z0-9-]{0,62}` nor `<YYYY-MM-DD>` ISO dates are checked anywhere in `tree::validate`. A path like `me/relationship/facts/Bad Slug.md` walks through silently.
- **Forbidden plaintext under encrypted tiers** — spec §5.1 reserves `encrypted/` for ciphertext. `relative_memory_paths` happily picks up `.md` files anywhere, including under `encrypted/`. There is no plaintext-detection check. This is the safety boundary that protects sensitive data after a botched merge or a hand-edit.
- **Unknown top-level directories** — `relative_memory_paths` will accept `random_dir/foo.md`. Spec §5.1 enumerates the allowed top-level dirs; the validator must reject the rest.

The path-prefix list lives in `model.rs:565-579` (`validate_repo_relative_path`) but is _only_ consulted when `RepoPath::try_new` is called. The tree-walk validator builds `RepoPath` via `RepoPath::new` (`validate.rs:43-45`), which _skips_ validation. The "safe relative path" gate exists but isn't wired into the validator.

**Fix:** in `validate_tree`, call `RepoPath::try_new(rel)` (or factor a shared `validate_repo_path` helper) and propagate the error. Add an `is_under_encrypted_tier(path)` check that errors when a plaintext `.md` parses successfully under `encrypted/`.

### B4. `repair_duplicate_ids` returns success while leaving the tree in an inconsistent state

`crates/memory-substrate/src/ids/repair.rs:46-48`

Even setting B1 aside: `repair_duplicate_ids` calls `validate_tree(repo, FullySynced)` _after_ renames (`repair.rs:47`). If references weren't rewritten (per B1.2) the validator will either hit `MissingReference` and return `Err(...)` — meaning the function returns an error after the filesystem has been mutated and the original duplicates have been deleted (`repair.rs:42`) — _or_ it'll succeed only if no references exist, which never happens in a real repo. There is no rollback. **Fix:** after fixing B1, the function still needs either (a) staged writes that can be undone on validation failure, or (b) explicit atomicity contract documented and acceptance-tested. As written, an error from the post-validation step leaves the repo half-renamed and unrecoverable.

### B5. `validate_repo_relative_path` allow-list and `tree::layout::memory_dirs` disagree

`crates/memory-substrate/src/model.rs:565-579` vs `crates/memory-substrate/src/tree/layout.rs:6-34`

Two sources of truth for which directories are valid Stream A paths. `memory_dirs` lists 22 nested directories under 11 top-level prefixes (`me/`, `projects/`, `agent/`, `dreams/`, `substrate/`, `encrypted/`, `tombstones/`, `events/`, `policies/`, `leases/`). `validate_repo_relative_path` lists exactly those 10 prefixes plus `.gitattributes`/`.gitignore`/`config.yaml`.

That's tolerable today, but `substrate/` per spec §5.1 contains _device-sharded JSONL_ files, not `.md` files; `events/` and `tombstones/` are also JSONL-only. The path validator currently allows `.md` files under any of those, and the tree validator will pick them up via `relative_memory_paths`. Spec invariant: those tiers should _never_ contain plaintext memory files. **Fix:** narrow the allowed-as-memory prefixes to `me/`, `projects/`, `agent/`, `dreams/`, `encrypted/`. Reserve `substrate/`, `tombstones/`, `events/`, `policies/`, `leases/` for non-memory writes only and reject `.md` files there explicitly.

---

## Risks

### R1. Default `EmbeddingTriple` returned when `config.yaml` is missing

`crates/memory-substrate/src/config/mod.rs:74-78, 125-127`

`load_synced_config` returns `SyncedConfig::default()` when `config.yaml` is absent, which calls `default_embedding()` → `EmbeddingTriple { provider: "synthetic", model_ref: "stream-a-test", dimension: 32 }`. Spec invariant 3 (CLAUDE.md, spec §10.2.2): `(provider, model_ref, dimension)` is identity, no silent fallback. A fresh clone or a deleted `config.yaml` will silently route every embedding write at the synthetic 32-dim test triple, which Stream B and the index will then accept as "real." **Fix:** make `active_embedding` a required field on `SyncedConfig` and return a typed error when missing. Move the synthetic triple to test fixtures only.

### R2. `seq.json` write is atomic against torn writes but leaks the lock file on crash

`crates/memory-substrate/src/ids/sequence.rs:46-128`

`fs::create_dir_all` → open `seq.lock` with `lock_exclusive` → operate → drop file = unlock. The `seq.lock` file is never deleted; that's fine. But two issues:

1. `OpenOptions::new().create_new(true)` for the temp file (`sequence.rs:120`) means a stale `seq.json.tmp` from a previous crash blocks the next allocation — `create_new` errors with `AlreadyExists`. The error is swallowed as `IdError::InvalidState(io)` with no recovery hint. **Fix:** if a `.tmp` exists at the start of a locked allocation, treat it as a leftover and remove it before opening with `create_new`. Document the assumption that the exclusive lock guarantees no concurrent writer holds the temp file.
2. The fsync sequence is correct (file → rename → parent dir fsync via `File::open(runtime).and_then(|d| d.sync_all())`), but on Windows `File::open(dir)` then `sync_all()` is a no-op for directories. Spec §3.1 doesn't require Windows, so this is informational, not blocking.

### R3. `parse_frontmatter_yaml` shape failure messages are lossy

`crates/memory-substrate/src/frontmatter/parse.rs:34-37, 88-94`

When `serde_json::from_value` fails to deserialize the assembled object into `Frontmatter`, the resulting error is wrapped as `BadShape(format!("frontmatter: {err}"))`. The serde error message is preserved, but `read_required` (`parse.rs:88-94`) discards it entirely (`map_err(|_| BadShape(field))`). For `scope` or `sensitivity` typos the user gets "bad shape for sensitivity" with no hint that they wrote `secrert` instead of `secret` (or, more importantly, that `secret` is rejected because it's not a valid persisted enum). **Fix:** preserve the deserializer message in `BadShape` or introduce a `BadEnum { field, value, message }` variant. The `BadEnum` variant exists in `error.rs:138-139` but is unused in this slice.

### R4. `validate_frontmatter` packs nine cross-field rules into one function

`crates/memory-substrate/src/frontmatter/validate.rs:117-153`

`validate_cross_fields` is 36 lines and bundles eight unrelated rules (self-reference, supersession overlap, supersede→status coupling, tombstone events, prospective extras, sensitivity↔retrieval coupling, privacy_scan, …). Each rule emits `ValidationError::BadShape(<adhoc string>)`, which loses the structured rule taxonomy from spec §9.2 (`CrossFieldViolation { rule, fields }`). The validator is correct as a one-shot, but the moment a rule fails in production, the operator gets `bad shape for "supersession overlap"` instead of `CrossFieldViolation { rule: "supersession_no_overlap", fields: ["supersedes", "superseded_by"] }`. **Fix:** introduce `ValidationError::CrossFieldViolation { rule: &'static str, fields: Vec<&'static str> }` and split each `if` into a named helper. The function will get longer, but each failure point will be testable and operator-readable.

Spec §9.1 also requires "Cross-field pass collects all applicable errors." The current implementation short-circuits at the first failure (`return Err(...)`), so a malformed file with three independent cross-field violations only ever shows one. **Fix:** accumulate violations into a `Vec<CrossFieldViolation>` and return `Err(ValidationError::CrossFieldViolations(vec))` only after the pass completes.

### R5. Two `SUPPORTED_SCHEMA_VERSION` constants

`crates/memory-substrate/src/frontmatter/schema.rs:4` and `crates/memory-substrate/src/merge/mod.rs:11`

Both literal `1`. CLAUDE.md invariant 5 names `MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` as the merge driver's source of truth, but the frontmatter validator has its own constant. The day someone bumps the schema, half the codebase will go and half won't. **Fix:** define one constant in `frontmatter::schema`, re-export from `merge`, and make `MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` an alias. Acceptance test: `assert_eq!(SUPPORTED_SCHEMA_VERSION, MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION)`.

### R6. `serialize_frontmatter` emits YAML by hand

`crates/memory-substrate/src/frontmatter/serialize.rs:35-86`

This module reimplements YAML emission instead of using `yaml_serde` (or its serializer), and `plain_yaml_string` accepts any ASCII alphanumeric + `_-./:@ ` as plain. That's mostly fine, but:

- A summary containing exactly `null` / `true` / `false` / `~` / `yes` / `no` will round-trip as a YAML keyword instead of a string. `plain_yaml_string("null")` returns true → emitted as `summary: null`. On reparse, `summary` becomes `Value::Null`, fails the required-string check. Round-trip stability for benign-looking inputs is broken.
- Numeric-looking strings (`"123"`, `"1e10"`, `"0xff"`) would parse back as numbers. The same hazard applies.
- Spec §6.13 requires "Round-trip canonical serialization is byte-stable." Today, that holds only for non-keyword, non-numeric inputs. **Fix:** quote strings that match YAML reserved literals or the YAML 1.1/1.2 numeric grammar; safest is to defer to `yaml_serde::to_string` for the actual leaves and only own the key-ordering layer.

### R7. `parse_device_id` (api.rs) is a hand-rolled YAML parser bypassing `LocalDeviceConfig`

`crates/memory-substrate/src/api.rs:798-811` (out of slice but consumes `LocalDeviceConfig` from `config/mod.rs`)

`Substrate::open` doesn't go through `config::load_local_device_config`. Instead it reads `local-device.yaml` line-by-line, splits on the first `:`, looks for keys named `device_id` or `id`, and takes the _first_ match. With the canonical layout `device:\n  id: dev_xxx`, the parser hits `device:` first (key=`device`, value=empty) → falls through, then hits `id: dev_xxx` (key=`id`) → returns `dev_xxx`. Works. But with any line in the file matching `id:` _anywhere_ — including under `paths:` or in a comment — this picks the wrong value. The `config::LoadedConfig.local.device.id` already has the strongly-typed value. **Fix:** in `Substrate::open`, replace `read_or_create_device_id` with `load_config(...).local.unwrap().device.id`. The "create if missing" branch should be in `git::adopt_clone`, not in `open`.

Calling this out here because it makes `LocalDeviceConfig` (in-slice) effectively unused at the open path, and because spec invariant 4 ("A fresh clone must regenerate device identity via `git::adopt_clone` before any write") is violated by `open` silently auto-creating a device id.

### R8. `next_memory_ids` does not enforce date monotonicity

`crates/memory-substrate/src/ids/sequence.rs:88-100`

If the system clock jumps backward (NTP correction, container restart, etc.) `today_utc < state.date`, `read_state` does _not_ trip the date-mismatch path (`if state.date != today` only equates by string equality). The next allocation will mint IDs for the _earlier_ date. Spec §7.2 step 4 says "If date changed, set `date=today_utc`, `next=max(1, max_existing_sequence(today_utc, local_shard)+1)`." This handles forward jumps; backward jumps produce IDs whose `YYYYMMDD` is later than today, which violates the §7.4 acceptance signal "10,000 sequential IDs on one device are unique and monotonic by sequence" if the dates aren't monotonic. **Fix:** if `parsed_date > today`, refuse with a typed error (`IdError::ClockRegression { stored, observed }`) rather than overwriting state.

---

## Nits

### N1. `parse.rs:30` uses `as_object_mut` but doesn't validate the map shape twice

`parse_frontmatter_yaml` reads the YAML root, mutates it via `materialize_defaults`, _clones_ it for `serde_json::from_value`, and then re-inspects the clone for extras. The clone is wasteful for large memories with extras. Restructure as: deserialize once into `Frontmatter` (the now-defaulted serde_json `Value`), then drain the original map of canonical keys to get extras without cloning.

### N2. `validate.rs:11-17` regex `expect_used` justification is fine but the comment line is on the wrong line

The trailing comment `// expect-justified: ...` is on the same line as the `expect("valid regex")` call, which is correct for `clippy` but bumps the line over 100 chars. Move the comment above the line. (Style only.)

### N3. `defaults.rs:6-16` could just be `Source::default()`

`Source` doesn't derive `Default`, but adding it would make `default_source()` redundant. Same for `WritePolicy::default()`. The current functions are fine but they're shadowed by the standard idiom.

### N4. `serialize_document` re-validates frontmatter

`crates/memory-substrate/src/frontmatter/serialize.rs:11-15`

`validate_frontmatter` is called inside `serialize_document`. That couples serialization to validation, which means any caller that holds a `Memory` and wants a string can't get one without the validator running. For the canonical write path that's fine; for tooling (e.g. `tree::repair_duplicate_ids` that already trusts its memories) it's redundant work. Document the contract or split into `serialize_unchecked` + `serialize_document`.

### N5. `tree/validate.rs:96-136` clones `supersedes_edges` to merge the inverse

`semantic_edges = supersedes_edges.clone()` then mutates. For repos with thousands of memories this is fine, but the cycle detector could just take both edge maps and walk them together without the clone. Defer until profiling justifies it.

### N6. `frontmatter/mod.rs:9` re-exports `parse_frontmatter_yaml`

The function is exported but every caller in the slice goes through `parse_document`. If `parse_frontmatter_yaml` is genuinely public API (per spec §16.2 `parse_frontmatter`), keep it; otherwise drop the re-export to shrink the surface.

### N7. `repair.rs:35-44` hand-builds a string from a `Path`

```rust
memory.path = Some(RepoPath::new(new_relative.to_string_lossy().replace('\\', "/")));
```

`new_relative.to_string_lossy()` then replacing backslashes is brittle. `RepoPath::try_new` already exists in `model.rs`; use it (and surface the error rather than constructing an unvalidated `RepoPath`).

### N8. `config/mod.rs:9-26` `SyncedConfig::default()` and `default_embedding` are paired but live apart

Move `default_embedding` next to `SyncedConfig::default` or inline it. The two-line free function buys nothing.

---

## Strengths worth keeping

- **`secret` is structurally impossible to persist.** `Sensitivity` enum (`model.rs:69-78`) has no `Secret` variant; serde will reject `sensitivity: secret` at parse time before `validate_frontmatter` even runs. Plus `validate_cross_fields` (`validate.rs:144-148`) refuses `index_body`/`index_embeddings` for confidential/personal. CLAUDE.md invariant 1 is enforced at the type level — exactly the right shape.
- **Required scalar/object fields are checked in `parse_frontmatter_yaml` before defaulting** (`parse.rs:54-55`). `scope` and `sensitivity` cannot fall to defaults that would silently change indexing posture. This is the right ordering.
- **`shard_for_device` (`sequence.rs:28-31`) is a one-line truth function.** Hashing is centralized; the test fixture and the runtime use the same code path.
- **Sequence allocator is exclusive-locked, fsynced, and uses parent-dir fsync** (`sequence.rs:46-128`). The atomic-write discipline matches spec §3.1 / §7.2.
- **`validate_supersession_graph` separates inverse-mismatch from cycle detection** and routes mismatches through `record_inverse_mismatch` so `PartialSync` warnings vs `FullySynced` errors fall out of one helper. Clean shape.
- **Lifecycle/trust pair is a pure function with an exhaustive match** (`validate.rs:52-69`). No `_` arm; adding a new `MemoryStatus` variant will break the build, which is what you want.
- **`ParsedMemory` carries warnings alongside the typed memory** (`parse.rs:12-18`) — the warning-vs-error split spec §9.2 mandates is naturally expressed in the return type rather than a side channel.

---

## Open questions for Trey

1. **`Substrate::open` auto-creating device IDs vs spec invariant 4.** `api.rs:785-795` regenerates `dev_<uuid>` on the fly when `local-device.yaml` is absent, then writes it. Spec invariant 4: "A fresh clone must regenerate device identity via `git::adopt_clone` before any write." Today `git::adopt_clone` (`git/adopt.rs:10-26`) doesn't generate a device ID at all — it just creates dirs and configures the merge driver. Either (a) move device-id minting into `adopt_clone` and have `open` fail with `OperatorRepairRequired` when missing, or (b) update the spec to acknowledge that `open` is the device-identity authority. Out of my slice, but it changes how `config/mod.rs::LocalDeviceConfig` is used.
2. **Survivor selection in `repair_duplicate_ids` needs git access.** Per B1.1, the spec wants `git commit timestamp` as a tiebreaker. Should repair take a `git::Repo` handle, or should it fall back to `(created_at, device_id, path)` when git history is unavailable (e.g. fresh init with hand-staged duplicates)? My read of spec §7.3 is that git-aware is mandatory for the typical flow but the test surface should cover the no-history case.
3. **Plaintext-under-`encrypted/` detection scope.** B3 calls for the tree validator to refuse plaintext `.md` under `encrypted/`. But ciphertext under `encrypted/` is _also_ `.md`-suffixed per spec §5.1 ("`encrypted/me/...`"); detection has to be content-shape aware, not extension-aware. Should the validator require an `encryption:` frontmatter field (or a magic ciphertext envelope) on every file under `encrypted/`, or is this Stream D's job that Stream A merely refuses to write? Worth pinning before fixing.
4. **`StartupPreflight` is only useful if `git::preflight` runs from inside `validate_tree`.** Today `validate_tree` doesn't depend on `git::*`. Either we plumb a git handle through (gross) or we move merge-driver-config checks out of tree validation entirely and let `Substrate::open` orchestrate the calls. The latter is cleaner; if so, drop `StartupPreflight` from the enum and document `open`'s checklist.
5. **YAML round-trip safety in `serialize_frontmatter`.** R6 above. Cheapest fix is to always quote string scalars; that costs canonical-form purity but eliminates a footgun. Want a structural fix or are we OK with "don't put the string `null` in your summary"?
