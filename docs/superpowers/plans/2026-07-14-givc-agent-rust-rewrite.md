# GIVC Agent Rust Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Rust agent runtime scaffold in a new `crates/agent` crate, then move the CLI and config loading into Rust without changing the proto contract.

**Architecture:** Put the agent in its own Rust crate at `crates/agent`. Split the rewrite into small Rust modules: a bootstrap/runtime layer, a typed CLI layer, and a typed config layer. The bootstrap will later host the controller ports, but this first phase only creates the skeleton that later services can plug into.

**Tech Stack:** Rust 2024, `tokio`, `clap`, `serde`, `serde_json`, `tonic`, `tonic-reflection`, `tracing`

## Global Constraints

- Keep protobuf and RPC contracts unchanged.
- Preserve Go semantics during the migration window.
- Keep the old Go agent code available as reference until the rewrite is complete.
- Use `procfs` for `/proc` access in `statsmanager` later.
- Each step should land in its own commit when practical.

---

### Task 1: Agent Boilerplate

**Files:**
- Create: `crates/agent/Cargo.toml`
- Create: `crates/agent/src/lib.rs`
- Create: `crates/agent/src/runtime.rs`
- Create: `crates/agent/src/service.rs`
- Create: `crates/agent/src/bin/givc-agent.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `givc::trace_init()`, existing tonic/tls helpers, shared proto re-exports.
- Produces: an `AgentRuntime` entrypoint and a stub gRPC service that can return `tonic::Status::unimplemented(...)` for unfinished RPCs.

- [ ] Create the module tree and export points.
- [ ] Add a runtime struct that owns future agent state but only wires startup/shutdown placeholders for now.
- [ ] Add a stub `SystemdService`-style server wrapper for the future agent services.
- [ ] Build: `rtk cargo check -p givc-agent`
- [ ] Commit: `docs(agent): add rust agent boilerplate`

### Task 2: CLI

**Files:**
- Create: `crates/agent/src/cli.rs`
- Modify: `crates/agent/src/lib.rs`
- Modify: `crates/agent/src/bin/givc-agent.rs`

**Interfaces:**
- Consumes: the boilerplate runtime from Task 1.
- Produces: a typed `Cli` struct and a `parse()`-based binary entrypoint.

- [ ] Move the agent command-line flags into a dedicated `Cli` type.
- [ ] Keep existing flag names and environment variable names unchanged.
- [ ] Wire the binary to call the new CLI parser, but keep runtime behavior equivalent.
- [ ] Build: `rtk cargo check -p givc-agent`
- [ ] Commit: `feat(agent): move givc-agent cli to rust`

### Task 3: Config

**Files:**
- Create: `crates/agent/src/config.rs`
- Modify: `crates/agent/src/runtime.rs`
- Modify: `crates/agent/src/bin/givc-agent.rs`

**Interfaces:**
- Consumes: the CLI type from Task 2.
- Produces: a typed config loader that builds the agent runtime state from JSON.

- [ ] Add a Rust config model matching the Go JSON shape.
- [ ] Implement config load and derived fields that the current Go code fills in.
- [ ] Keep behavior stable for service name, endpoint, TLS, and capability expansion.
- [ ] Build: `rtk cargo check -p givc-agent`
- [ ] Commit: `feat(agent): add rust config loading`

## Review Gates

- Run `rtk cargo test -p givc` after the three commits.
- Keep the Go implementation untouched for reference until the Rust agent passes parity checks.
