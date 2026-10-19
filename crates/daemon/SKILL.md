# daemon skill

## Purpose

The crate has the D1 run contract: one contract for runs that the CLI and the UI share. `serve` (`src/serve.rs`) is the `fh serve` command. It runs `Daemon` until a stop signal, opens its page in the browser (`src/browser.rs`), and stops a running daemon with `--stop`. `crates/cli` only calls it. Its page (`src/page.html`, HTML and plain JS) has the chat on the left and the wasm viewer of `crates/viewer` in the center. It runs the agent of the chat (`src/chat.rs`, `docs/chat.md`).

## Inputs and outputs

- `Service` keeps the store, the run queue and the event queues. `Daemon` adds the HTTP transport on 127.0.0.1 and writes `daemon.info` (port and token, mode 0600).
- `Client` is the client of the contract for the CLI and the tests. `Client::discover(STATE_DIR)` reads `daemon.info`.
- Commands: submit, preview, cancel, read a record, list the runs, read the events.
- The text form, the routes and the access rules are in `src/contract.rs`, `src/http.rs` and `docs/run-contract.md`.
- A run starts `ccx` through `fh_solver_ccx::Ccx` in the daemon process, on a read-only copy of its input and of the files that it includes (`*INCLUDE`). The limits of a run (`timeout`, `memory`) come with the submit.
- `src/chat.rs` runs 1 agent (Claude Code, `claude -p --input-format stream-json --output-format stream-json`) with the instructions of `src/agent.md`, and keeps its actions as chat entries for `/api/v1/chat`. The agent stays alive between the messages, and each message goes to its stdin, also while it works.

## Invariants the code must keep

- Bind to 127.0.0.1 only (DESIGN.md section 4.1).
- Every request carries the token. A request with an `Origin` that is not `http://127.0.0.1:<port>` fails. The `Host` must be `127.0.0.1:<port>`.
- Each run has one record. A final state never changes.
- A run solves its copy of the input, never the file of the client.
- The queues and the event log are bounded (`MAX_QUEUED`, `EVENT_QUEUE`, `EVENT_LOG`).
- The solver never lives longer than the daemon. A later daemon shows each run that an earlier daemon did not end as `interrupted`. It never starts such a run again.
- One daemon at a time uses a state folder (`daemon.lock`).
- The browser never holds an API key (DESIGN.md section 4.1).
- femahoy has no agent loop of its own. The agent is a separate harness process (DESIGN.md section 5.1). A chat entry never holds the token.
- The token never goes into the command line of a browser process that stays alive. `$BROWSER` and `xdg-open` get `STATE/open.html` (mode 0600). `--no-open` and `FH_NO_OPEN` stop the open.

## Forbidden actions

- Bind to any address other than 127.0.0.1 (DESIGN.md section 4.1).
- Send a CORS header that lets another origin read an answer.
- Change the solve path from this crate. It is in `crates/solver-ccx`.

## Validation hooks it must pass

Run `cargo check -p fh-daemon` and `cargo test -p fh-daemon`. `tests/serve.rs` tests `fh serve`: no token, a wrong token, a wrong `Origin`, loopback only and the stop. `tests/contract.rs` has the D1 exit checks: browser close, CLI reconnect, cancel, the copy of the input, a daemon stop and a daemon failure (SIGKILL), the foreign `Origin` and the authorized CLI. `tests/viewer.rs` tests the page and its layout, the viewer files, the run list, the result files and the preview, each with the token and `Origin` rules. `tests/chat.rs` tests the chat routes with a fake agent that prints a recorded event stream and takes a message during its work. No test calls a real LLM. `crates/cli/tests/run.rs` tests the browser open, `--no-open`, `FH_NO_OPEN` and `--stop` through `fh`.

## Examples

```rust
let daemon = Daemon::start_with_options(Config { state_dir, solver: Ccx::new() }, 0, Options::default())?;
let cli = Client::discover(&state_dir)?;
let record = cli.submit(&root.join("model.inp"), &Limits::default())?;
```
