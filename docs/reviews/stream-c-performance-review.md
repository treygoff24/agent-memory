# Stream C Performance Review

**Scope:** Stream C governance latency/resource review. Read-only against production code; this report is the only repository file written.

**Reviewed focus areas:** policy load caching, repeated full-tree scans, `query_chunks`/top-K costs, JSONL tombstone parsing, Substrate/index lock contention, and daemon request timeouts.

## Baseline and profiling evidence

Commands run from `/Users/treygoff/Code/agent-memory` on the current dirty Stream C tree:

| Probe                                                                                                                                                                                      | Result                                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/usr/bin/time -lp cargo test -p memory-governance`                                                                                                                                        | pass; `real 0.49s`; max RSS `242106368` bytes                                                                                                                                                                    |
| `/usr/bin/time -lp cargo test -p memoryd governance`                                                                                                                                       | pass; `real 0.60s`; max RSS `55853056` bytes                                                                                                                                                                     |
| `/usr/bin/time -lp cargo test -p memoryd review_queue`                                                                                                                                     | pass; `real 0.40s`; max RSS `56049664` bytes                                                                                                                                                                     |
| `BENCH_RUNS_OVERRIDE=3 BENCH_CORPUS_OVERRIDE=200 /usr/bin/time -lp bash scripts/bench-gate.sh --tier smoke --profile stream-c-review --output /tmp/stream-c-performance-review-bench.json` | pass; Stream A 200-corpus baseline: cold reindex p50 `138.059ms`, FTS chunk query p50 `0.081ms`, vector chunk query p50 `0.138ms`, tree validator p50 `41.704ms`; process `real 5.93s`, max RSS `17072128` bytes |

I also ran a temporary probe outside the repo under `/tmp/stream-c-perf-probe` that seeded a substrate and called the real `memoryd::handlers::handle_request` paths. No production files were changed.

|       Corpus | `ReviewQueue { limit: Some(10) }` | governed `WriteMemory` |  `Search` |
| -----------: | --------------------------------: | ---------------------: | --------: |
| 200 memories |                        `75.659ms` |             `84.157ms` | `0.243ms` |
| 500 memories |                       `146.033ms` |            `158.984ms` | `0.195ms` |

The dominant measured bottleneck is not SQLite FTS/vector lookup; it is Stream C's daemon-side file-tree scan and Markdown parse before review/write decisions.

## Findings

### P1 - SC-PERF-001 - Governed write, supersede, and review queue do full-tree Markdown scans on the hot path

**Evidence:**

- `crates/memoryd/src/handlers.rs:185-187` loads policy and then calls `active_memory_summaries(substrate).await?` for every governed write before building the engine.
- `crates/memoryd/src/handlers.rs:213-217` repeats the same full active-memory load for every supersede request.
- `crates/memoryd/src/handlers.rs:413-423` implements review queue by iterating all memory paths, reading each envelope, building the full queue, and only then applying `limit`.
- `crates/memoryd/src/handlers.rs:522-543` implements `active_memory_summaries` by walking every relative memory path, reading each envelope, filtering active plaintext records, and cloning summaries into a vector.
- `crates/memory-substrate/src/tree/layout.rs:79-93` shows `relative_memory_paths` is a full `walkdir::WalkDir::new(root)` over all Markdown files.

**Impact:** This makes common Stream C operations O(number of Markdown files) plus O(parse all memory frontmatter/body) even when callers ask for a bounded queue or one governed write. The temp probe shows this directly: at only 500 memories, review queue is `146.033ms` and governed write is `158.984ms`, while indexed search is `0.195ms`.

**Suggested benchmark:** Add a Stream C daemon benchmark that seeds 100, 1k, and 10k memories with active/quarantined/candidate ratios, then records p50/p95/p99 and max RSS for:

1. `memory_write` with an explicit user source.
2. `memory_supersede`.
3. `memory_review_queue --limit 10`.
4. `memory_search` as the indexed control.

Fail the benchmark if `review_queue(limit=10)` reads more records than needed after an index-backed review projection exists, or if governed write grows linearly with total corpus when only a small top-K candidate set is needed.

### P1 - SC-PERF-002 - Daemon in-flight requests have no execution timeout or blocking isolation

**Evidence:**

- `crates/memoryd/src/server.rs:60-64` documents that in-flight requests already read from the socket are handled to completion.
- `crates/memoryd/src/server.rs:128-153` only wraps frame reads in the shutdown/read select; after a frame is decoded, `handle_request(&dispatch, request).await` has no timeout.
- `crates/memoryd/src/server.rs:180-188` applies `idle_frame_timeout` only to `reader.fill_buf()`, so it protects silent clients, not slow handlers.
- `crates/memoryd/src/client.rs:14-30` connects, writes, and reads without a client-side timeout.
- `crates/memory-substrate/src/api.rs:30-35` stores the index behind `Arc<Mutex<Index>>`; `crates/memory-substrate/src/api.rs:276-282` and `crates/memory-substrate/src/api.rs:841-850` run write/query index work under that mutex.

**Impact:** Any slow full-tree scan, filesystem stall, SQLite lock wait, or future provider-backed governance check can pin the request indefinitely. Because the server spawns connection tasks but executes synchronous filesystem and SQLite work directly in async handlers, tail latency can also spill into unrelated clients when Tokio worker threads are busy.

**Suggested benchmark:** Add a daemon concurrency benchmark with 32 concurrent clients:

1. One client runs a deliberately slow `review_queue` or injected slow `SimilaritySearch`.
2. The other clients issue `Status`, `Search`, and small `WriteMemory` requests.
3. Record p95/p99 latency and timeout/error shape.

The acceptance target should include an explicit per-request timeout response and prove status/search remain responsive under one slow governance request. Also instrument lock wait/hold time around `Substrate.index` before changing the locking model.

### P2 - SC-PERF-003 - Policy files are reloaded and YAML-parsed on every governed write/supersede

**Evidence:**

- `crates/memoryd/src/handlers.rs:185` calls `load_policy_set(substrate.roots().repo.as_path())` for governed write.
- `crates/memoryd/src/handlers.rs:215` repeats it for supersede.
- `crates/memoryd/src/handlers.rs:491-508` does an initial `read_dir`/extension scan and then either loads policies or falls back to built-ins.
- `crates/memory-governance/src/policy.rs:130-143` `PolicySet::load_from_dir` performs another directory scan and reads every `.yaml`.
- `crates/memory-governance/src/policy.rs:294-299` reads each policy file and parses YAML.

**Impact:** Small today, but this turns policy selection into repeated filesystem and YAML work on every write-like request. It also becomes a tail-latency problem if policies live on a synced filesystem or if policy count grows.

**Suggested benchmark:** Seed policy directories with 4, 40, and 400 policy YAML files and measure p50/p95/p99 for `memory_write` with:

1. Current per-request load.
2. A cached `PolicySet` stored in daemon state.
3. Cached `PolicySet` plus mtime/hash invalidation.

The benchmark should report policy load count per request and fail if steady-state governed writes read policy files after a cache warmup.

### P2 - SC-PERF-004 - Stream C top-K contradiction detection is not index-backed and does not push limits into retrieval

**Evidence:**

- `crates/memoryd/src/handlers.rs:522-543` builds the entire active-memory summary set from disk before top-K selection.
- `crates/memoryd/src/handlers.rs:551-564` implements `MemorydSimilaritySearch` by linearly searching the in-memory vector for exact hashes and returning the first `limit` entries for `top_k`.
- `crates/memory-governance/src/contradiction.rs:335-340` always calls `search.top_k(candidate, self.top_k_limit)` and then invokes the tiebreaker if any hit is above threshold.
- `crates/memory-substrate/src/index/query.rs:177-203` shows the FTS path already has a fixed `LIMIT 20`; `crates/memory-substrate/src/api.rs:841-850` exposes `query_chunks` without a caller-provided limit and hard-codes vector queries to `20`.

**Impact:** Stream C pays for a full disk scan before it can ask for top-K, then the daemon implementation does not actually rank by similarity. The existing Stream A smoke bench shows index-backed FTS/vector queries are sub-millisecond on a 200-memory corpus, so Stream C is bypassing the cheaper path.

**Suggested benchmark:** Add a top-K benchmark that seeds 1k/10k/50k active records with controlled entity/hash distributions and compares:

1. Current full-scan `active_memory_summaries` + `MemorydSimilaritySearch::top_k`.
2. Index-backed exact duplicate lookup by canonical claim/entity hash.
3. Index-backed FTS/vector top-K with caller-provided K.

Track number of Markdown files read, number of SQLite rows scanned, provider/tiebreaker invocations, p95 latency, and RSS. The target should be O(log/index query + K), not O(total Markdown files).

### P2 - SC-PERF-005 - Tombstone JSONL loading/matching is whole-file and linear; daemon state has no cache boundary

**Evidence:**

- `crates/memory-governance/src/tombstone.rs:134-145` loads all `.jsonl` files into one `Vec<TombstoneRule>`.
- `crates/memory-governance/src/tombstone.rs:147-149` matches by scanning `self.rules.iter().find(...)`.
- `crates/memory-governance/src/tombstone.rs:222-231` reads each JSONL file with `fs::read_to_string` and then parses every non-empty line.
- `crates/memoryd/src/handlers.rs:514-518` currently constructs the daemon engine with `TombstoneIndex::default()`, so there is no long-lived daemon cache/invalidation seam yet.

**Impact:** The parser is fine for tiny fixtures, but it is not safe as a future hot-path implementation. A naive fix that loads tombstones per write would read and parse the entire tombstone corpus every request; keeping it as a long-lived `Vec` still makes every match linear in tombstone count.

**Suggested benchmark:** Add a tombstone benchmark with 1k, 10k, and 100k rules spread across multiple JSONL files:

1. Cold load time and max RSS.
2. Hot match p50/p95/p99 for target-id hits, content/entity-hash hits, and misses.
3. Malformed-line fail-closed latency for large files.

Target a cached/indexed representation keyed by `target_memory_id` and `(content_hash, entity_hash)`, plus file mtime/hash invalidation. Also set a maximum JSONL line size or stream parse to avoid one huge line forcing whole-file allocation.

### P2 - SC-PERF-006 - Existing tests prove behavior but not scale, bounds, or contention

**Evidence:**

- `crates/memoryd/tests/governance_e2e.rs:7-211` covers governed write/supersede/forget/review behavior with tiny temp repositories.
- `crates/memoryd/tests/review_queue.rs:11-47` covers review queue with one quarantined memory.
- `crates/memory-governance/tests/tombstone_contract.rs:27-32` covers tombstone matching with one fixture match.
- Existing timeout tests cover frame/shutdown behavior, but not slow handler execution or client request deadlines.

**Impact:** The current test suite can stay green while hot paths remain O(total memories), queue limits are applied after full scans, and daemon clients can wait forever on a slow handler.

**Suggested benchmark/test additions:**

- `memoryd` scale test marked ignored by default: seed 1k memories, call `ReviewQueue { limit: Some(10) }`, assert bounded latency and bounded reads once an index-backed projection exists.
- `memoryd` concurrent request test: one slow review/governance request must not cause `Status` p95 to exceed a small bound.
- `memory-governance` tombstone scale benchmark: compare Vec scan against hashed indexes.
- Stream C bench gate output should live outside immutable Stream A baselines until the threshold is accepted.

## Severity summary

- **P0:** None found in the performance lane.
- **P1:** 2 findings.
  - Full-tree Markdown scans dominate governed writes/supersede/review queue.
  - Daemon has no in-flight request timeout or blocking isolation.
- **P2:** 4 findings.
  - Policy reload/YAML parse happens per write-like request.
  - Top-K contradiction retrieval is not index-backed and does not push limits.
  - Tombstone JSONL loading/matching needs a cache/index before it is placed on the hot path.
  - Existing tests lack scale, timeout, and contention coverage.
