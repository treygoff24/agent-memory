# Clean-code review — markdown / events / git

Reviewer: reviewer-io-git
Date: 2026-04-25
Spec anchor: `docs/specs/stream-a-core-substrate-v1.1.md`

Files reviewed:

- `crates/memory-substrate/src/markdown/mod.rs`
- `crates/memory-substrate/src/markdown/atomic.rs`
- `crates/memory-substrate/src/markdown/cas.rs`
- `crates/memory-substrate/src/markdown/durability.rs`
- `crates/memory-substrate/src/events/mod.rs`
- `crates/memory-substrate/src/events/log.rs`
- `crates/memory-substrate/src/events/framing.rs`
- `crates/memory-substrate/src/events/recovery.rs`
- `crates/memory-substrate/src/git/mod.rs`
- `crates/memory-substrate/src/git/init.rs`
- `crates/memory-substrate/src/git/command.rs`
- `crates/memory-substrate/src/git/commit.rs`
- `crates/memory-substrate/src/git/sync.rs`
- `crates/memory-substrate/src/git/preflight.rs`
- `crates/memory-substrate/src/git/adopt.rs`

Cross-referenced (out of slice but relied upon):

- `crates/memory-substrate/src/tree/layout.rs` — `bootstrap_repo_tree` writes the `.gitattributes` content this slice depends on.
- `crates/memory-substrate/src/api.rs` — call sites for `auto_commit`, event log path computation.

Headline: this slice has at least four genuine BLOCKERs (one of them silent-data-corruption nasty), several spec deviations that look like over-simplifications rather than malice, and a lot of error/argv plumbing that does not yet match the contract Codex graded itself against. It is not a release-certification candidate.

---

## Blockers

### B1. `events/recovery.rs:14-37` — UTF-8-lossy decoding can corrupt the log on truncation

`recover_event_log` reads the file as bytes and then immediately calls `String::from_utf8_lossy(&bytes)`. Every invalid UTF-8 byte is replaced by `U+FFFD` (3 bytes UTF-8 encoded). The function then sums `line.len()` over the lossy `String` and uses the result as a **byte offset into the original file** (`file.set_len(valid_end as u64)`).

Concrete failure mode: a crash mid-write that truncates inside a multi-byte sequence yields invalid UTF-8 in the trailing line. The lossy decode inflates the apparent byte length of every preceding character that was already valid (it does not — only invalid bytes inflate), but the trailing partial bytes get replaced with `U+FFFD` (3 bytes) for each invalid byte. So `valid_end` computed against the lossy string is _not_ the same byte offset as in the on-disk file once any prior line ever contained a malformed UTF-8 character. We then `set_len(valid_end)` at the wrong place, silently truncating valid earlier events or leaving stray bytes from the malformed trailing line in place.

This violates spec §12.3 step 5 by silently corrupting otherwise-recoverable logs.

Fix: do not detour through `String::from_utf8_lossy`. Either:

- iterate `bytes` directly, splitting on `b'\n'` and treating any non-UTF-8 line as malformed (route through `decode_line(std::str::from_utf8(line).ok()?)`), accumulating `valid_end` from byte slice lengths;
- or read line-by-line via `BufRead` and track the byte offset returned by `Read::take` / `BufReader::stream_position` after each successful decode.

Also: after `set_len`, the file is not fsynced and the parent directory is not fsynced. A crash between truncation and the next event append will undo the recovery on some filesystems. Add `file.sync_all()` and a parent-dir fsync (the helper from `markdown::atomic::fsync_dir` should be hoisted to a shared `io::fsync_dir`).

### B2. `events/log.rs:88-110` — `refuse_duplicate_device_logs` device extractor is fragile and wrong

`refuse_duplicate_device_logs` derives the device name from the file stem with `stem.split([' ', '.', '(']).next()`. This is supposed to detect a copied same-device log per spec §12.5 ("Same-device-id duplicate logs from a bad clone are detected and refused until adoption repair").

Problems:

1. The spec's invariant is "no two log files for the same device id". The implementation picks `'.'` as a delimiter but the file stem (already stripped of the `.jsonl` extension) only contains a `.` if the ULID/UUID portion contains one (it should not) **or** if a copy tool produced something like `dev_abc.copy` and the loop strips on `.copy`. But device IDs as shipped by `api.rs:43` are `dev_<uuid::simple>` — no dots. So the `.` split is dead. The `' '` / `(` splits handle macOS Finder duplication (`dev_abc (1)`, `dev_abc copy`). That is a narrow heuristic; e.g. Linux `cp` produces `dev_abc.jsonl.bak` which yields stem `dev_abc.jsonl` (extension check would reject — good) but also `dev_abc-1.jsonl` which yields stem `dev_abc-1`, a _new_ device per this heuristic — not flagged.
2. More importantly: this scan does not enforce the spec's actual invariant. The spec says device IDs are unique per _adopted clone_ and live only in `~/.memoryd/local-device.yaml` (§13.1.5, §13.2.1). The check should be "exactly one log file whose name matches the local device id, plus zero-or-more logs for known peer devices" — that requires reading `local-device.yaml` to know which id belongs to _this_ machine. As coded, it can't tell self-copy from a legitimate peer log.

Fix: pass the local device id in. Refuse only when there are multiple files whose stem matches `<local_device_id>` plus any suffix; everything else with a stable id is a peer log and is fine. Alternative shape:

```rust
pub fn refuse_duplicate_device_logs(events_dir: &Path, local_device_id: &str) -> std::io::Result<()> { … }
```

This is a correctness blocker because today the function will both miss real duplicates (Linux suffix patterns) and false-positive on legitimate multi-device clones (any peer log whose name contains `' '`, `'.'`, or `'('` is collapsed to a shared key — e.g. you could conceivably have two peer device IDs named such that they share the prefix-before-paren).

### B3. `events/framing.rs` and `log.rs` — framing format does not match spec §12.1

Spec §12.1 example:

```json
{
  "schema": 1,
  "id": "evt_01HX...",
  "ts": "...",
  "device": "dev_a1b2...",
  "seq": 42,
  "kind": "WriteCommitted",
  "data": {},
  "crc32c": "..."
}
```

The CRC32C is a **field inside the JSON object**, alongside `schema`, `device`, `seq`, etc. The implementation in `framing.rs:6-10` instead emits `"{checksum:08x} {json}\n"` — an out-of-band hex prefix.

Even setting aside the CRC placement: the encoded JSON does not include `schema`, `device`, `seq`, or the spec-mandated typed `data` substructure. The `Event` struct (`log.rs:14-25`) has only `id`, `operation_id`, `at`, `kind` — no `device`, no `seq`, no `schema`. Spec §12.1 explicitly requires `seq` to be per-device monotonic, persisted under exclusive lock at `~/.memoryd/event-seq.json`, and to be the basis for ordering display unions. None of that exists.

Two-clone convergence (spec §13.6.1) defines set equality of events keyed by `id`, so this hasn't broken convergence yet, but multi-device union display (§12.4 "union all `events/*.jsonl` by `(ts, device, seq, id)`") cannot work without `device` and `seq` in the record.

This is a blocker because the on-disk format is stamped today and changing it later means a schema migration. Fix:

- Add `schema`, `device_id`, `seq` to `Event`, stored inside the JSON object.
- Move `crc32c` into the JSON object (or document why we are deviating; out-of-band framing is defensible but the spec example is normative).
- Persist `~/.memoryd/event-seq.json` under exclusive lock; bump after fsync per §12.1.
- Bound line length to 64 KiB per §12.3 step 1; today there is no length check on append.

If the deviation from the spec's literal layout is intentional, bump the spec or add a §12.1.1 explaining the alternate framing. Either way, fix the missing fields.

### B4. `git/init.rs:13-29` and `tree/layout.rs:41` — `.gitattributes` does not match spec §13.1

Spec §13.1 step 2 mandates `.gitattributes`:

```gitattributes
* text eol=lf
*.md merge=memory-frontmatter-merge
events/*.jsonl merge=union
substrate/**/*.jsonl merge=union
tombstones/*.jsonl merge=union
```

`tree/layout.rs:41` writes only:

```gitattributes
*.md merge=memory-merge-driver
```

Three problems:

1. The driver name is `memory-merge-driver`. Spec §13.1 step 6 calls it `memory-frontmatter-merge`. `init_git_repo` and `adopt_clone` configure git for `merge.memory-merge-driver.driver`; the `.gitattributes` references the same name — internally consistent. But spec §14.1 inputs use the same convention as §13.1 (`memory-frontmatter-merge`). Either the spec name or the code name should change; today they are out of sync.
2. `events/*.jsonl`, `substrate/**/*.jsonl`, `tombstones/*.jsonl` all need `merge=union`. None are configured. Without these, JSONL merges run the merge driver (which only knows `*.md`) or fall back to the default text driver and produce conflict markers. That breaks §13.5 and §13.6.
3. `* text eol=lf` is missing; without it, Windows checkouts will normalize CRLFs and break canonical-content equality (§13.6.1).

Spec §13.6.1 explicitly relies on byte-identical event-log set equality and exact byte equality for `tombstones/**` and `substrate/**/*.jsonl`. That is impossible without the JSONL union attribute on every JSONL path the spec lists, and without `eol=lf` enforcement.

Fix: emit the full `.gitattributes` content from §13.1 step 2 in `tree/layout.rs::bootstrap_repo_tree`. Reconcile driver name across spec §14.1, `tree/layout.rs`, `git/init.rs`, and `git/adopt.rs`.

### B5. `git/init.rs:28` and `git/commit.rs:11` — silent commit failure

Both call sites use `let _ = run_git(repo, &["commit", …])` to swallow the result. The intent is presumably "no-op if there is nothing to commit," because `git commit` exits non-zero when the index is clean. Today this also silently swallows real failures: pre-commit hook rejection, signing failure, locked index, missing user.email, you name it.

Spec §13.4 step 4 says auto-commit emits `GitCommitted` after the commit; step 5 says append. The current implementation can return `Ok(())` when no commit was made at all, with no event emitted, no warning, nothing.

Fix: distinguish "nothing to commit" from "commit failed for a real reason." Either inspect `git status --porcelain` first and skip the commit if clean (deterministic), or parse stderr/exit-code (e.g. exit 1 + stderr "nothing to commit, working tree clean" is the benign case). Anything else is a real error and should propagate.

### B6. `git/commit.rs:10` — `git add -A :/` is too broad and conflicts with §13.4 step 3

Spec §13.4 step 3: "`git add -A` only inside repo root." `:/` is the pathspec for "from repo root," which is fine in spirit, but `-A` then stages **everything** under that — including paths the spec explicitly says should not be tracked: any stray temp files (`.<basename>.<op_id>.tmp`), runtime metadata mistakenly placed in the repo, half-written merge-driver outputs, etc.

The substrate-wide invariant is "device IDs live only in local runtime state, never in synced `config.yaml`" (system invariant 1). Today nothing prevents `auto_commit` from staging a stray `local-device.yaml` if a future bug or operator slips it into the repo root (`.gitignore` says `/.memoryd/` but the spec/api code stores `local-device.yaml` _inside_ `roots.runtime` which is supposed to be `~/.memoryd/`; if anyone ever runs with `runtime == repo` for testing — and `api.rs::write_local_device_id` does not enforce otherwise — then `auto_commit` will leak the device id into the synced commit).

Fix:

- Restrict the staged paths to the namespaces the spec actually tracks (`me/`, `agent/`, `projects/`, `dreams/`, `substrate/`, `encrypted/`, `tombstones/`, `events/`, `policies/`, `leases/`, `config.yaml`, `.gitattributes`, `.gitignore`). Anything else should be skipped or surfaced as a refuseable foreign change.
- Validate that `roots.runtime` is not inside `roots.repo` at `Substrate::open` / `adopt_clone` time. (May already exist elsewhere; if not, add it.)
- Verify temp files (`.<basename>.<op_id>.tmp`) are in `.gitignore` — today the gitignore is `/.memoryd/\n*.sqlite\n*.sqlite-wal\n*.sqlite-shm\n` (`tree/layout.rs:42`); the temp file pattern is _not_ covered. A crash mid-write will leave a temp file the next `auto_commit` will happily `git add`.

Add `/.*.tmp` (or more specific pattern) to the gitignore content.

### B7. `git/sync.rs:14-18` — `fetch_and_merge` violates spec §13.5

Spec §13.5:

```
1. Preflight.
2. git fetch origin.
3. Compute ahead/behind/diverged exactly.
4. If only ahead: no merge.
5. If behind or diverged: git merge --no-ff origin/main.
6. If git exits conflict due to true textual/unparseable conflicts, stop and surface.
7. Scan for valid `status: quarantined` memories and append `MergeQuarantined` events.
…
11. Append `GitFetched`.
```

Implementation:

```rust
pub fn fetch_and_merge(repo: &Path) -> Result<(), GitError> {
    run_git(repo, &["fetch"])?;
    run_git(repo, &["merge", "--ff-only", "@{u}"])?;
    Ok(())
}
```

Mismatches:

1. No preflight call — spec §13.5 step 1 requires it. (`preflight.rs::git_preflight` exists but is not invoked here.)
2. `--ff-only` instead of `--no-ff origin/main`. `--ff-only` will hard-fail any diverged history that the merge driver is supposed to resolve — i.e. it converts the entire happy path of the system into a fatal error.
3. No ahead/behind computation, no `MergeQuarantined` event scan, no `GitFetched` event append, no auto-commit of reconciliation work.
4. `@{u}` instead of `origin/main`. Acceptable substitute _only_ if upstream tracking is configured; spec is explicit about `origin/main`.

This is the central git happy path; it does not work as specified.

### B8. `markdown/atomic.rs:84-122` — suppression-ledger bookkeeping has lost-update windows

Two issues:

1. `if let Ok(mut ledger) = suppression.lock()` silently ignores poisoned mutexes (lines 85, 109, 116). If a panic poisoned the ledger lock, we will skip the `insert_in_flight` step, do the rename, then skip the `promote_committed` step. The watcher then sees a notify event for the rename with no suppression entry and re-ingests the file we just wrote, opening the very race the in-flight ledger was created to close (spec §8.3 step 9, §16.5).
2. The order in success path is: `insert_in_flight` (line 86) → `write_temp_file` → `rename` → `fsync_dir` → `promote_committed` (line 110). On failure between `rename` and `fsync_dir`, the file is already at its final path; we then skip `promote_committed` and call `ledger.remove(&relative)` (line 117). The watcher now sees a real notify for a file that is on disk and has NO suppression entry. False reingest.

Fix:

- Use `.lock().expect("suppression ledger poisoned")` or a dedicated error path; do not silently ignore poisoning.
- `promote_committed` must happen as soon as the rename completes (after step 10 of §8.3 — before parent-dir fsync), with the `expires_at` set such that even if step 11 fails the watcher knows to suppress the rename event. Alternatively, the in-flight entry must be allowed to remain until the operation either commits or expires by timeout.

Spec §8.3 step 12 places `promote_committed` after parent-dir fsync, so the strict ordering is intentional, but then spec footnote line 682 says the in-flight entry "expires if the process dies or the write aborts." The current code removes the in-flight entry on error (line 117) instead of letting it expire. That is closer to what we want for the rename-failed case but wrong for the fsync-dir-failed case (the file is already on disk). Distinguish by where in the closure we failed.

---

## Risks

### R1. `markdown/atomic.rs:23-30` — re-canonicalizing the repo on every read is slow and racy

`read_memory_file` calls `repo.canonicalize()` and `absolute.canonicalize()` for every read. The first is wasted work after the first call (the repo root does not change). The second can fail spuriously if the file was just renamed by a concurrent writer between `repo.join(...)` and `canonicalize`. Cache the canonical repo on `Substrate::open` and pass it in. Use `std::fs::canonicalize` only on the parent directory chain that already exists.

### R2. `markdown/atomic.rs:139-164` — `ensure_write_parent_contained` walks components but does the wrong thing on missing intermediates

The loop terminates the moment it hits `ErrorKind::NotFound`. That is correct for "we will be creating these intermediates." But there is no symlink check for the chain _after_ the first non-existent segment. Code path:

1. `repo/me/relationship/preferences/foo.md` — last component is `foo.md`, intermediate dirs already exist.
2. Replace `me` with a symlink pointing outside the repo while we're here. Now `current.canonicalize` on `repo/me` returns the outside path. The function correctly catches that.
3. Replace `me/relationship/preferences` with a symlink to outside. Same deal.
4. But: someone replaces a leaf-adjacent dir `preferences/` with a symlink between this check and the eventual `fs::create_dir_all(parent)` (line 93). The function approves the chain; `create_dir_all` follows the symlink. TOCTOU.

Spec §6.6 lists path-safety as an invariant (no symlinks, no traversal). The current check is best-effort. Fully closing the TOCTOU requires `O_NOFOLLOW` on each component (`openat(dirfd, ".", O_DIRECTORY|O_NOFOLLOW)` style). That is a meaningful refactor; flag rather than blocker because adversarial filesystems are out of the v1.0 threat model unless the spec disagrees.

Mid-term, lift this into a helper that returns a `RepoPath -> Vec<DirHandle>` so the same fd chain is reused for both the parent-creation and the rename. Use `nix` or `rustix` openat APIs.

### R3. `markdown/atomic.rs:96` — temp filename suffix collision risk

`format!(".{file_name}.{}.tmp", args.operation_id.as_str())` — if two concurrent writers pass the same `operation_id` (e.g. through caller bug or replay), `OpenOptions::new().create_new(true)` correctly fails the second one. But the resulting `WriteFailureKind::Io` lets the caller retry without distinguishing "another write is in flight" from "disk full." Surface this as a typed `WriteFailureKind::ConcurrentInFlight` so callers can backoff vs fail-fast.

### R4. `markdown/durability.rs:8-20` — durability probe collapses real failures to `BestEffort`

```rust
match std::fs::File::open(root).and_then(|file| file.sync_all()) {
    Ok(()) => DurabilityTier::Full,
    Err(err) if matches!(err.kind(), std::io::ErrorKind::Unsupported) => DurabilityTier::BestEffort,
    Err(_) => DurabilityTier::BestEffort,
}
```

Spec §3.1 implies `Refused` for genuine failures. EACCES, ENOSPC, EIO on the open or fsync should return `DurabilityTier::Refused`, not `BestEffort`, because the operator should not silently lose durability guarantees — they should be told the storage is unhealthy. Fix:

- Only `ErrorKind::Unsupported` (or platform equivalents like ENOTSUP / EPERM on directories on certain FUSE filesystems) maps to `BestEffort`.
- Other errors map to `Refused`.

### R5. `events/log.rs:88-110` — directory iteration is O(n) and not lock-protected

`refuse_duplicate_device_logs` scans `events/` on every startup. That's fine. But if the daemon is running and a copy tool drops in a second log file _during_ operation, this check never re-runs. Either schedule a periodic re-check or have the watcher fire a re-validation when files appear under `events/`. Spec §13.2 puts adoption-time on the operator; v1.0 ok, but flag for §16/§17 follow-up.

### R6. `git/command.rs:9-23` — `run_git` uses `current_dir(repo)`, ignoring HOME/XDG and brittle on Windows

`Command::new("git").args(args).current_dir(repo)` inherits the parent process environment. That means a malicious `GIT_EXEC_PATH`, `GIT_CONFIG_NOSYSTEM=0` (the default unset is "use system config"), `GIT_DIR` or `GIT_WORK_TREE` from the parent shell can override our intent. Spec §13.1 step 6 lists explicit local-config keys; those are stored in `.git/config` and survive env override only because we pass explicit `git -c` or absolute `--git-dir`. We do not.

For a v1 server-side daemon this is a moderate threat; for a CLI tool inheriting a developer environment it can break commits silently. Fix:

- Always pass `git -c core.autocrlf=false -c pull.rebase=false ...` rather than relying on `.git/config` from prior `init`; or
- Sanitize the environment in `run_git` (clear `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_NAMESPACE`, etc.).
- Use absolute path to `git` discovered at startup, not the ambient `PATH`.

Spec §13.1 footnote (line 1332) says "The driver command uses an absolute path or a stable shim path managed by installation. Ambient `PATH` is not sufficient for unattended merges." Same logic should apply to the substrate's invocation of git itself.

### R7. `git/preflight.rs` and `init.rs` — preflight is wired up but never invoked

`git_preflight` exists; nothing in `git/sync.rs::fetch_and_merge` or `git/sync.rs::push` calls it. Spec §13.5 step 1 requires preflight before fetch+merge. Already covered as part of B7 but worth flagging that the function itself is a stub: it only checks `.git` exists and the merge driver binary exists. It does not check `.gitattributes` content, local device id presence, working-tree quarantine state, or any of the other items spec §13.3 enumerates.

### R8. `events/log.rs:54-58` — `append_event` does not bound line length

Spec §12.3 step 1: "Max line length: 64 KiB; larger payloads must be artifacted and referenced." `append_event` happily writes any size. If a future event payload bug produces a 10 MiB line, recovery will choke on the resulting file (and so will every reader). Add a size check before write.

### R9. `events/log.rs:55` — `OpenOptions::new().create(true).append(true).open(path)` race after `create_dir_all`

Between `create_dir_all(parent)` (line 52) and `open(...)` (line 55), the parent directory could be removed by adversarial action. Resulting error is `ErrorKind::NotFound`, surfaced as a generic IO error. Acceptable for v1.0 if we trust the runtime, but for crash-recovery completeness consider opening the parent dir fd once at startup and using `openat`.

### R10. `git/adopt.rs:12-23` — silently uses `current_exe()` for merge driver path

`std::env::current_exe().unwrap_or_else(|_| "memory-merge-driver".into())` — if `current_exe()` fails (rare; chrooted, busybox, etc.) we fall back to the bare name `"memory-merge-driver"`, which gives us back the `PATH`-based lookup the spec explicitly forbids (§13.1 footnote line 1332). And: the substrate binary's `current_exe()` is the substrate, not the merge driver. Unless they are the same binary (spec does not require that), this is configuring git to invoke the substrate as a merge driver.

Fix: take the merge-driver path as a parameter (`init_git_repo` already does — `merge_driver_binary: &Path`); pipe it through `adopt_clone` the same way. Drop the `current_exe()` fallback entirely.

Spec §13.2 step 4 ("Configure local merge driver and git settings") expects deterministic config, not auto-discovered.

---

## Nits

### N1. `markdown/atomic.rs:57-124` — `atomic_write` is 67 lines of mixed abstraction levels

This function does, in order: path validation, repo containment check, precondition enforcement, serialization, hash computation, ledger insertion, parent-dir creation, temp-file naming, IIFE for the rename block, ledger promotion, ledger rollback. Eleven concerns in one function. Stepdown rule violation.

Reasonable shape:

```rust
pub fn atomic_write(args: AtomicWrite<'_>) -> Result<Sha256, WriteFailure> {
    let plan = plan_write(&args)?;        // path validation, encrypted-namespace check, precondition
    let serialized = serialize_with_hash(args.memory)?;
    let _suppression = SuppressionGuard::insert(args.suppression, &plan, &serialized)?;
    durably_replace_file(&plan, &serialized, args.durability)?;
    Ok(serialized.hash)
}
```

### N2. `markdown/atomic.rs:39-54` — `AtomicWrite` is a 7-field arg struct with mixed lifetimes

Eight fields (counting the lifetime), three of them optional. Ergonomics: `AtomicWrite::builder(repo, memory, op_id).durability(...).suppression(...).build()` would read better and avoid the repeated `args.foo` boilerplate inside the function.

### N3. `events/log.rs:50-58` — `append_event` mixes "ensure parent" with "append"

The `create_dir_all` is here because callers don't always set up the events dir. That is `bootstrap_repo_tree`'s job. Move the directory-creation responsibility to bootstrap and let `append_event` assume the parent exists; failure to find parent then becomes a real error (as it should).

### N4. `events/log.rs:60-85` — `read_events` bails on any malformed line

The function is named `read_events` (plural, valid events) but returns `Err` on the first malformed line. That conflicts with `recover_event_log` which is supposed to be the malformed-line handler. As a reader, this should either:

- silently drop malformed lines and return what we could read (with a warning), or
- delegate to `recover_event_log` first, then read.

Today, `read_events` is unusable on a log that has any malformed line, even after recovery would have fixed it. Make the contract explicit: `read_events_strict` vs `read_events_after_recovery`, or compose them at the call site.

### N5. `events/framing.rs:6-10` — `unwrap_or_else(|_| "{}".to_string())` hides bugs

If `serde_json::to_string(value)` fails we silently emit `"{}"` and a CRC of the empty object. Any caller relying on the round-trip will see `decode_line` succeed but `from_value::<Event>` fail later, far from the original bug. Either propagate the error (preferred — the function should return `Result<String, serde_json::Error>`) or panic; do not silently corrupt.

### N6. `git/init.rs:17-23` and `git/adopt.rs:14-22` — duplicated merge-driver config block

Both files write the exact same two `git config` calls. Extract `git::configure_merge_driver(repo, &merge_driver_binary)`; call it from both. Cuts 12 lines and ensures any future change touches one place.

### N7. `git/sync.rs:21-23` — error wrap is asymmetric

`fetch_and_merge` returns `GitError` directly; `push` wraps it in `GitError::GitPushFailed(err.to_string())`. If we want a "push specifically failed" variant, fine, but then `fetch_and_merge` should similarly wrap with `GitError::FetchFailed` / `GitError::MergeFailed`. Today the call site cannot tell whether the `fetch_and_merge` error came from fetch or merge.

### N8. `git/commit.rs:9-13`, `sync.rs:14-23`, `init.rs:10-30` — function bodies are 1-3 lines of pass-through

Most of `git/` is one-line wrappers around `run_git`. That is fine for testability but the abstraction is barely paying for itself. Either consolidate (`enum GitOp { … } fn perform(repo, op)`) or make each wrapper add real value (e.g. `fetch_and_merge` should add the §13.5 logic; once it does, the wrapper is justified — see B7).

### N9. `markdown/atomic.rs:11` — `unused_imports`-adjacent

`use crate::watcher::SuppressionLedger;` is used only in the field type. Fine; just noting that a dependency on `watcher` from `markdown` is a layering inversion (markdown is lower-level than watcher). Push the suppression-ledger interaction to the caller (`api.rs`) — let `atomic_write` return the final hash and the rename path; `api.rs` does the ledger transition. Cleaner module DAG.

### N10. `markdown/atomic.rs:127-133` — `remove_file_if_exists` reimplements `fs::remove_file` with `NotFound` swallowed

Standard idiom:

```rust
pub fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}
```

The `path.exists()` precheck is racy (the file may be removed between `exists` and `remove_file`) and adds an extra syscall. The above is one syscall and race-free.

### N11. `markdown/durability.rs:12` — `force_unsafe` parameter name

`force_unsafe` reads like a Rust `unsafe` block flag. It is in fact an opt-in to `BestEffort`. Rename to `allow_best_effort` to match the field name in `WriteRequest::allow_best_effort_durability`.

### N12. `markdown/cas.rs:8-12` — `hash_bytes` allocates a string on the hot path

For every write, `format!("sha256:{}", hex::encode(...))`. The `Sha256` type wraps a `String`; if it accepts `Cow<'_, str>` or stores `[u8; 32]` and renders to string only on serialization, the hot path becomes allocation-free. Out of slice, but flag for the data-model layer.

### N13. `events/log.rs:14-25` — `Event` field naming

`at: DateTime<Utc>` — spec §12.1 calls this `ts`. Inconsistent across emit/persist/serialize. Pick one; serde rename if you want the in-Rust identifier to differ from the on-disk key.

---

## Strengths worth keeping

- `markdown/atomic.rs:97-105` — the IIFE around the rename closure means the ledger rollback path on line 116 is unreachable from the success path. Closure-ifying the fallible block to centralize cleanup is the right pattern; do not lose it during the refactor in N1.
- `markdown/atomic.rs:67-75` — refusing plaintext writes targeting `encrypted/` is the correct early-fail point and matches §8.4.
- `events/recovery.rs` — the _intent_ (single-trailing-line policy) tracks spec §12.3 step 5 exactly. The implementation has the bug noted in B1 but the rule expressed by the code is right.
- `markdown/atomic.rs:139-164` — symlink-traversal hardening on the parent chain is more than most code does. Keep it; harden further per R2.
- The module decomposition (`markdown/`, `events/`, `git/` each with internal mod files exposing a small API) is clean. The clean-code "small files, narrow public surface" instinct is on display.

---

## Open questions for Trey

1. **Spec vs code naming for the merge driver.** Spec §13.1 uses `memory-frontmatter-merge`; code uses `memory-merge-driver` consistently. Which is canonical? The fix for B4 depends on which one we keep. If `memory-merge-driver` is the new chosen name, the spec needs a v1.2 errata; if `memory-frontmatter-merge` is canonical, a code rename is required. Either way it should land before §17 release certification.
2. **Out-of-band CRC vs in-JSON CRC** (B3). The spec example places `crc32c` inside the JSON object. Codex's framing puts a hex prefix outside it. The latter is faster to validate (no JSON parse to verify framing) and slightly safer (the CRC covers the full encoded payload independent of JSON canonicalization), but it deviates from the spec example. Want me to draft a §12.1.1 spec amendment that endorses the prefix framing, or should Codex move the CRC into the object?
3. **`refuse_duplicate_device_logs` semantics.** Should the function be parameterized on `local_device_id`, or should it be removed and replaced with a stricter check inside `git::adopt_clone` that asserts "exactly one log file matches my device id"? B2 fix sketch assumes the former; the spec wording (§12.5 acceptance signal) is ambiguous.
4. **Durability tier for failures other than "Unsupported"** (R4). Spec §3.1 distinguishes `Refused` from `BestEffort` but the code maps all probe failures to `BestEffort`. Trey, what's your read on the operator UX here — would you rather see "your storage is broken, fix it" (Refused) or "we'll write best-effort and let you opt in" (BestEffort)?
5. **Auto-commit safety net** (B6). Should `auto_commit` enforce a per-namespace allowlist (refuse to stage paths outside the §6.4 layout), or is that better handled by the watcher / index sync layer? My gut says the substrate should refuse foreign paths in `auto_commit` itself — the failure mode of staging foreign content is too painful to leave to a higher layer.

---

## Bottom line

`atomic.rs` is the strongest file in the slice: the rename-into-place protocol is structured correctly, the precondition checks work, and the symlink-parent walk is more careful than typical. The biggest correctness blockers are:

- **B1** (event-log recovery silently corrupts via UTF-8 lossy decoding),
- **B3** (event framing / record shape does not match spec §12.1; missing `device`, `seq`, `schema`),
- **B4** (`.gitattributes` is missing `events/*.jsonl merge=union`, `tombstones/*.jsonl merge=union`, `substrate/**/*.jsonl merge=union`, and `* text eol=lf` — breaks two-clone convergence),
- **B7** (`fetch_and_merge` is `--ff-only` instead of the §13.5 protocol; no preflight, no ahead/behind, no quarantine scan, no `GitFetched` event).

These four are showstoppers for §17.7 release certification regardless of how clean the rest of the implementation looks. The slice is closer to "happy path under test fixtures" than to "release-certification candidate."
