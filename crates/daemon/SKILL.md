# daemon skill

## Purpose

The crate has the D1 run contract: one contract for runs that the CLI and the UI share. `serve` (`src/serve.rs`) is the `fh serve` command. It runs `Daemon` until a stop signal, opens its page in the browser (`src/browser.rs`), and stops a running daemon with `--stop`. `crates/cli` only calls it. Its page loads the wasm viewer of `crates/viewer`. The daemon also owns the terminal sidebar: the PTY of each sidebar shell, the pane backend (herdr or plain PTYs) and the WebSocket of each terminal (`docs/terminal.md`).

## Inputs and outputs

- `Service` keeps the store, the run queue and the event queues. `Daemon` adds the HTTP transport on 127.0.0.1 and writes `daemon.info` (port and token, mode 0600).
- `Client` is the client of the contract for the CLI and the tests. `Client::discover(STATE_DIR)` reads `daemon.info`.
- Commands: register a project, submit, cancel, read a record, list the runs, read the events.
- The text form, the routes and the access rules are in `src/contract.rs`, `src/http.rs` and `docs/run-contract.md`.
- A run starts the protocol v2 solver (`fh-solve`) through `fh_solver_api::SubprocessSolver` on a fixed input revision.
- `src/pty.rs` runs a program in a PTY with `alacritty_terminal`. `src/screen.rs` encodes its screen in the protocol `fh.term.v1`, and `src/ws.rs` sends it on the WebSocket.
- `src/panes.rs` has the `PaneBackend` trait and the pty backend. `src/herdr.rs` has the herdr backend. The routes `/api/v1/terminals` and `/api/v1/panes` use them.

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
- Each terminal and pane route, the WebSocket included, has the same token and `Origin` rules as the other routes. Only the WebSocket takes the token in the subprotocol `fh.token.<token>`.
- Each herdr call gives `--session <name>` with the session that femahoy owns. The session is never `default`. femahoy never calls a server-wide herdr command (`server ...`, `session stop`, `session delete`).
- A herdr call and a PTY program get no `HERDR_*` variable from the daemon environment.
- `alacritty_terminal` is pinned to an exact version and only `src/pty.rs` uses it (DESIGN.md section 11.1).
- Other code reads the panes from `PaneBackend`, never from herdr directly (DESIGN.md section 4.3).
- The token never goes into the command line of a browser process that stays alive. `$BROWSER` and `xdg-open` get `STATE/open.html` (mode 0600). `--no-open` and `FH_NO_OPEN` stop the open.
- Line budget: 7k lines, tests included (DESIGN.md section 11).

## Forbidden actions

- Bind to any address other than 127.0.0.1 (DESIGN.md section 4.1).
- Send a CORS header that lets another origin read an answer.
- Change the solve path or the subprocess protocol from this crate.
- Use the herdr session `default`, or a herdr call with no `--session`.
- Add a way into a terminal that does not have the token and `Origin` rules.
- Bundle herdr (DESIGN.md section 4.3).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-daemon` and `cargo test -p fh-daemon`. `tests/serve.rs` tests `fh serve`: no token, a wrong token, a wrong `Origin`, loopback only and the stop. `tests/contract.rs` has the D1 exit checks: browser close, CLI reconnect, a duplicate submission, cancel, a daemon stop and a daemon failure (SIGKILL), the foreign `Origin` and the authorized CLI. `tests/viewer.rs` tests the page, the viewer files, the project list and the result files, each with the token and `Origin` rules. `tests/terminal.rs` tests the terminal and pane routes and the WebSocket: the access rules, the shell, the pty backend and the herdr backend with a fake herdr. Its test `herdr_lab_session` runs the real herdr only when `FH_TEST_HERDR_SESSION` names a lab session (`docs/terminal.md`). `crates/cli/tests/run.rs` tests the browser open, `--no-open`, `FH_NO_OPEN` and `--stop` through `fh`, with `--panes pty`.

## Examples

```rust
let daemon = Daemon::start(Config { state_dir, solver: SubprocessSolver::new("fh-solve") }, 0)?;
let cli = Client::discover(&state_dir)?;
let project = cli.register(&root)?;
let ack = cli.submit(&project, &new_request_id(), "model.inp")?;
```
