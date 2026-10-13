# daemon skill

## Purpose

The crate has the D1 run contract: one contract for runs that the CLI and the UI share. `serve` (`src/serve.rs`) is the `fh serve` command. It runs `Daemon` until a stop signal. `crates/cli` only calls it. The viewer, the PTY, the pane backends and the WebSocket server are not built yet.

## Inputs and outputs

- `Service` keeps the store, the run queue and the event queues. `Daemon` adds the HTTP transport on 127.0.0.1 and writes `daemon.info` (port and token, mode 0600).
- `Client` is the client of the contract for the CLI and the tests. `Client::discover(STATE_DIR)` reads `daemon.info`.
- Commands: register a project, submit, cancel, read a record, list the runs, read the events.
- The text form, the routes and the access rules are in `src/contract.rs`, `src/http.rs` and `docs/run-contract.md`.
- A run starts the protocol v2 solver (`fh-solve`) through `fh_solver_api::SubprocessSolver` on a fixed input revision.

## Invariants the code must keep

- Bind to 127.0.0.1 only (DESIGN.md section 4.1).
- Every request carries the token. A request with an `Origin` that is not `http://127.0.0.1:<port>` fails. The `Host` must be `127.0.0.1:<port>`.
- Each run has one record. A final state never changes.
- A retry with the same request ID changes nothing and gets the outcome `replay`.
- A run solves its fixed revision, never the file in the project.
- The queues and the event log are bounded (`MAX_QUEUED`, `EVENT_QUEUE`, `EVENT_LOG`).
- The solver never lives longer than the daemon. At start, the daemon marks each active run of an earlier daemon `interrupted`, or `cancelled` when a cancel was pending. It never starts such a run again by itself.
- One daemon at a time uses a state folder (`daemon.lock`).
- The browser never holds an API key (DESIGN.md section 4.1).
- Line budget: 7k lines, tests included (DESIGN.md section 11).

## Forbidden actions

- Bind to any address other than 127.0.0.1 (DESIGN.md section 4.1).
- Send a CORS header that lets another origin read an answer.
- Change the solve path or the subprocess protocol from this crate.
- Describe the viewer or the pane backends as available.
- Bundle herdr (DESIGN.md section 4.3).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-daemon` and `cargo test -p fh-daemon`. `tests/serve.rs` tests `fh serve`: no token, a wrong token, a wrong `Origin`, loopback only and the stop. `tests/contract.rs` has the D1 exit checks: browser close, CLI reconnect, a duplicate submission, cancel, a daemon stop and a daemon failure (SIGKILL), the foreign `Origin` and the authorized CLI.

## Examples

```rust
let daemon = Daemon::start(Config { state_dir, solver: SubprocessSolver::new("fh-solve") }, 0)?;
let cli = Client::discover(&state_dir)?;
let project = cli.register(&root)?;
let ack = cli.submit(&project, &new_request_id(), "model.inp")?;
```
