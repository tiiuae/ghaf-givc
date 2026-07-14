## GIVC Agent Rust Rewrite Design

### Context
The repository already has a Rust admin side in `crates/admin`, including an existing `givc-agent` binary, tonic server helpers, TLS helpers, and shared proto re-exports. The current production agent runtime still lives in Go under `modules/`, with controllers and transport code split across `modules/pkgs/*` and the entrypoint in `modules/cmd/givc-agent`.

The rewrite goal is to move the agent runtime and controllers to Rust while keeping the gRPC/proto contract 1:1 with the current Go implementation. Integration tests already exist and are the main parity gate.

### Goals
- Rewrite the agent runtime in Rust.
- Keep protobuf and RPC contracts unchanged.
- Port controller logic one service at a time.
- Reuse the existing Rust admin-server patterns for tonic, TLS, reflection, and error mapping.
- Keep the old Go agent code available as reference until the rewrite is complete.

### Non-Goals
- No proto/API redesign during this migration.
- No compatibility bridge between Go and Rust agent runtimes.
- No broad refactor of unrelated Rust crates.
- No behavior changes beyond what is needed to preserve Go semantics.

### Target Shape
The Rust agent will live in the existing `crates/admin` crate and extend the current `givc-agent` binary.

The runtime will be split into:
- Binary/bootstrap layer: CLI, config load, logging, shutdown, listener setup, service registration.
- Transport layer: tonic server wiring and request/response mapping.
- Controller layer: domain logic for each service.
- Shared agent state: config, registry-like state, clients, and shutdown context.

The agent should expose the same proto services as the Go agent. Initial unimplemented methods should return tonic `UNIMPLEMENTED` errors, not panic, so the binary is runnable early while the rest of the services are still pending.

### Migration Order
1. Boilerplate and runtime skeleton.
2. `servicemanager`.
3. One remaining service at a time.
4. `statsmanager` using `procfs` for `/proc` access.

The order beyond `servicemanager` can be adjusted if a later controller turns out to be lower risk, but the rewrite will still proceed strictly one service per step.

### Runtime Design
The Rust bootstrap should own the following responsibilities:
- Parse the existing agent CLI/config surface.
- Build TLS and listener configuration.
- Start tonic server with reflection.
- Register services and interceptors.
- Handle shutdown cleanly.

The handler side should stay thin:
- gRPC methods validate and translate inputs.
- Controllers do the actual work.
- Transport modules keep the proto mapping isolated from the domain logic.

Error handling should follow the existing Rust admin-server style:
- use `anyhow` in internal layers,
- convert to tonic status at the boundary,
- prefer explicit `UNIMPLEMENTED` for unfinished RPCs,
- log enough context for parity debugging.

### Controller Design
Each controller port should preserve the Go structure as much as useful:
- `controller.rs` for domain behavior.
- `transport.rs` for tonic service implementation.
- Small helper modules only when they reduce coupling.

The intent is to make each controller easy to compare against the Go source during the migration window.

### `servicemanager`
This is the first real port.

It should establish the core Rust agent patterns:
- remote systemd operations,
- host/agent service registration behavior,
- request mapping and response shaping,
- shared state and client usage.

This controller becomes the template for later ports.

### `statsmanager`
Use `procfs` for reading system data.

Keep `/proc` parsing behind a dedicated stats provider module so the gRPC layer does not know filesystem details.

### Testing Strategy
- Keep integration tests as the main acceptance gate.
- Add or update unit tests for each Rust controller as it lands.
- Compare Rust behavior against existing Go behavior before removing the Go reference.
- For `statsmanager`, add focused tests around `/proc` parsing and missing-field cases.

### Commit Strategy
- One small commit for boilerplate/runtime scaffolding.
- One commit per controller whenever possible.
- If a controller needs multiple internal steps, split it into a few tightly related commits.

### Risks
- Hidden behavior in Go controllers may only show up in integration tests.
- `statsmanager` can be sensitive to kernel and container differences.
- The runtime split must stay small or the Rust agent will become harder to port than the Go one.

### Outcome
When this design is implemented, the Rust agent will be a drop-in replacement for the Go agent from a proto and integration-test perspective, and the remaining Go code will be only a temporary reference during the migration.
