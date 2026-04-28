# Stream B Daemon + MCP Implementation Plan

**Goal:** Build the first daemon-backed control plane for `agent-memory`: a local `memoryd` process, a socket protocol, agent-facing request handlers, and a CLI/MCP bridge foundation.

**Architecture:** Stream A remains the canonical storage and index substrate. Stream B adds a separate `memoryd` crate that owns process lifecycle, one Unix-socket JSON protocol, substrate-backed handlers, and background worker scaffolding. The MCP server is intentionally a thin forwarder over the same daemon protocol so every harness gets identical semantics.

**Tech Stack:** Rust workspace, `tokio` Unix sockets, newline-delimited JSON envelopes, `serde` DTOs, `clap` CLI, `memory-substrate` public API.

---

### Task 1: Workspace Crate And CLI Skeleton

**Parallel:** no
**Blocked by:** none
**Owned files:** `Cargo.toml`, `crates/memoryd/Cargo.toml`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/lib.rs`, `crates/memoryd/src/cli.rs`

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/memoryd/Cargo.toml`
- Create: `crates/memoryd/src/lib.rs`
- Create: `crates/memoryd/src/main.rs`
- Create: `crates/memoryd/src/cli.rs`
- Test: `crates/memoryd/tests/cli_contract.rs`

**Step 1: Write the CLI contract test**
Create an integration test that proves the binary accepts `memoryd --help`, exposes `serve`, `status`, `search`, and `get`, and keeps admin verbs separate from future MCP tool verbs.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memoryd cli_contract`
Expected: fail because the `memoryd` crate does not exist yet.

**Step 3: Add the crate and parser**
Add `memoryd` to the workspace. Implement a small `clap` parser with explicit `serve`, `status`, `doctor`, `search`, `get`, and `write-note` commands. Do not implement destructive admin commands in this task.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memoryd cli_contract`
Expected: pass.

### Task 2: Daemon Socket Protocol

**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/server.rs`, `crates/memoryd/tests/protocol_contract.rs`

**Files:**
- Create: `crates/memoryd/src/protocol.rs`
- Create: `crates/memoryd/src/server.rs`
- Test: `crates/memoryd/tests/protocol_contract.rs`

**Step 1: Write protocol tests**
Cover JSON round trips for `Status`, `Doctor`, `Search`, `Get`, and `WriteNote` request variants and success/error response envelopes.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memoryd protocol_contract`
Expected: fail before protocol types exist.

**Step 3: Implement newline-delimited JSON protocol**
Define `RequestEnvelope { id, request }`, `ResponseEnvelope { id, result }`, and bounded response bodies. Keep the shape agent-friendly: structured JSON, small snippets, and explicit `call memory_get for full body` guidance.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memoryd protocol_contract`
Expected: pass.

### Task 3: Substrate-Backed Request Handlers

**Parallel:** no
**Blocked by:** Task 2
**Owned files:** `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/handler_contract.rs`

**Files:**
- Create: `crates/memoryd/src/handlers.rs`
- Test: `crates/memoryd/tests/handler_contract.rs`

**Step 1: Write handler tests against temp roots**
Initialize a Stream A repo with `Substrate::init`, write one memory through Stream A, then verify `Search` and `Get` return bounded protocol responses.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memoryd handler_contract`
Expected: fail before handlers exist.

**Step 3: Implement handlers**
Map `Status`/`Doctor`/`Search`/`Get`/`WriteNote` to Stream A APIs. `WriteNote` should create candidate/substrate-safe records only; full governance remains Stream C.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memoryd handler_contract`
Expected: pass.

### Task 4: Local Daemon Serve Loop

**Parallel:** yes
**Blocked by:** Task 2
**Owned files:** `crates/memoryd/src/server.rs`, `crates/memoryd/tests/server_smoke.rs`

**Files:**
- Modify: `crates/memoryd/src/server.rs`
- Test: `crates/memoryd/tests/server_smoke.rs`

**Step 1: Write socket smoke test**
Start the server on a temp Unix socket, send a `Status` request, and assert one response line returns with the same request id.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memoryd server_smoke`
Expected: fail until server loop exists.

**Step 3: Implement accept loop**
Use `tokio::net::UnixListener`, spawn one task per connection, read one JSON line at a time, and write one JSON response line. Refuse oversized lines.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memoryd server_smoke`
Expected: pass.

### Task 5: Background Worker Scaffolding

**Parallel:** yes
**Blocked by:** Task 3
**Owned files:** `crates/memoryd/src/workers.rs`, `crates/memoryd/tests/worker_lifecycle.rs`

**Files:**
- Create: `crates/memoryd/src/workers.rs`
- Test: `crates/memoryd/tests/worker_lifecycle.rs`

**Step 1: Write worker lifecycle test**
Start worker handles with cancellation, trigger shutdown, and assert every worker exits cleanly.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memoryd worker_lifecycle`
Expected: fail before workers exist.

**Step 3: Implement stub workers**
Add named loops for watcher/indexer, embedding queue, sync manager, and MCP peer activity. They should expose lifecycle/health state without performing full Stream C/D/E behavior.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memoryd worker_lifecycle`
Expected: pass.

### Task 6: Thin MCP Forwarder Foundation

**Parallel:** no
**Blocked by:** Tasks 2-4
**Owned files:** `crates/memoryd/src/mcp.rs`, `crates/memoryd/tests/mcp_manifest.rs`

**Files:**
- Create: `crates/memoryd/src/mcp.rs`
- Test: `crates/memoryd/tests/mcp_manifest.rs`

**Step 1: Write manifest test**
Assert exactly seven agent-facing tools are declared: `memory_search`, `memory_get`, `memory_write`, `memory_supersede`, `memory_forget`, `memory_startup`, and `memory_note`.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memoryd mcp_manifest`
Expected: fail before MCP manifest exists.

**Step 3: Implement manifest and forwarding boundaries**
Create tool descriptors and request conversion functions. Do not add admin tools to MCP.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memoryd mcp_manifest`
Expected: pass.

### Verification

Run after each implemented slice:

```bash
cargo fmt --all -- --check
cargo test -p memoryd
cargo test --workspace
```

Run before Stream B closeout:

```bash
bash scripts/check.sh
cargo +nightly-2025-09-18 fuzz run merge_driver -- -max_total_time=600
```
