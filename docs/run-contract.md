# Run contract (D1)

The run contract is one contract for runs. The `fh` CLI and the UI both use it. The code is in `crates/daemon`. `fh serve` runs the daemon (see [`fh serve`](#fh-serve)). `fh run` is the CLI client (see [`fh run`](#fh-run)).

The solve path does not change. A run starts the protocol v2 solver (`fh-solve-ccx`, `docs/solver-protocol.md`) on a copy of the input file.

The contract has 4 commands for the chat and the CLI: start a run, show its progress, stop it and show its results. The page also shows a model before the solve (a preview).

## Words

| Word | Meaning |
|---|---|
| Run | One solve of one input file. Its ID is `run-000001`, `run-000002` and so on, in each state folder. A run has one record. At submit, the daemon copies the input file into the folder of the run (read-only). The run solves the copy, so a later edit of the file does not change the run. |
| Preview | A copy of an input file with no run. Its ID is `1`, `2` and so on. The page shows the model of the copy before the solve (`docs/chat.md`). A daemon removes the previews of an earlier daemon at start, because only the chat of the daemon that made a preview shows it. |

## Run states

```
queued -> running -> succeeded | failed | cancelling | interrupted
queued -> cancelled | interrupted
cancelling -> cancelled | succeeded | failed
```

`succeeded`, `failed`, `cancelled` and `interrupted` are final. A final state never changes.

## Access

The daemon listens on 127.0.0.1 only. It makes a random token at each start and writes the port and the token to `<state>/daemon.info` (mode 0600).

1. The `Host` header must be `127.0.0.1:<port>`.
2. A request with an `Origin` header must come from `http://127.0.0.1:<port>`, the page of the daemon. Any other `Origin` fails with 403 `origin.refused`.
3. Every request must carry `Authorization: Bearer <token>`. A client with no `Origin`, such as the CLI, needs only the token. Only `GET /`, the page of the daemon, can also take the token as `?token=<token>`, because a browser that opens a link sends no header.

Each answer has `Referrer-Policy: no-referrer`, so the page never sends its address with the token to another site.

## Messages and routes

A message is UTF-8 text with one `key=value` line for each field. An empty line ends the fields. Item lines can follow, for example the events of a run. Each message has `contract=2`. An error has the fields `error` and `message`.

| Method and path | Body | Answer |
|---|---|---|
| `GET /` | | the page of the daemon (HTML). It loads the wasm viewer (`docs/viewer.md`). |
| `GET /viewer/fh_viewer.js`, `GET /viewer/fh_viewer_bg.wasm` | | the wasm viewer. `viewer.missing` when it is not built. |
| `GET /api/v1/status` | | `pid` |
| `GET /api/v1/runs` | | one item `<run> <state> <input>` for each run |
| `POST /api/v1/runs` | `input` | the run record of the new run |
| `GET /api/v1/runs/<r>` | | the run record |
| `POST /api/v1/runs/<r>/cancel` | | the run record after the cancel |
| `GET /api/v1/runs/<r>/events?since=N&wait_ms=T` | | events after `N` |
| `GET /api/v1/runs/<r>/input` | | the bytes of the copy of the input that the run solves |
| `GET /api/v1/runs/<r>/files/<name>` | | the bytes of a file in the run folder `out/`. `name` is a plain file name (letters, digits, `.`, `_`, `-`, no `.` first), else `file.invalid`. A file that is not there gives `file.missing`. |
| `POST /api/v1/previews` | `input`, `title` | `preview`. The page shows the model of the preview before the solve (`docs/chat.md`). No run starts. |
| `GET /api/v1/previews/<id>` | | the bytes of the copy of the preview. `preview.unknown` when the preview is not there. |

The `input` of a submit or a preview is the absolute path of a file, else `input.invalid` (400).

A cancel stops a queued run at once (`cancelled`). A cancel of a running run gives `cancelling`, and the solver gets the protocol v2 `cancel`. A cancel of a final run changes nothing.

## Limits

| Limit | Value |
|---|---|
| Time of a run | No limit. A large model can solve for hours. A cancel stops a run. |
| Runs in the queue | 64. A submit after that fails with `queue.full` (503). |
| Events of a run in memory | 1024. A client that reads too late gets the count in `dropped`. |
| Event text | 1024 bytes. A longer text is cut. |
| Event log of a run | 1 MiB. Then the log gets one `log-full` line. State events are always written. |
| Events in one answer | 256 |
| Wait of an events request | 30 seconds |
| Connections at the same time | 32 |

The protocol v2 runner writes the full solver output to `out/stdout.txt`. The contract does not limit that file.

## Which process stays alive

The daemon owns every run. A browser and the CLI are only clients.

1. A client that closes or disconnects stops nothing. The run continues. The client reads the same record when it connects again.
2. Only a cancel command stops a run on purpose.
3. The solver never lives longer than the daemon. A daemon that stops cancels its runs, and they end as `interrupted`. A daemon that fails closes the stdin of the solver, and protocol v2 then stops the solver.
4. A later daemon shows a run that an earlier daemon did not end as `interrupted`, never as `running`. It never starts such a run again.
5. One daemon at a time uses a state folder. The lock file `daemon.lock` makes this sure.
6. `fh run` uses the daemon that answers for the state folder. If no daemon answers, `fh run` starts `fh serve --state DIR --no-open` in the background, in its own process group, with its output in `DIR/daemon.log`. This file has mode 0600, because it holds the page address with the token. That daemon stays alive when `fh run` ends, so the browser keeps the results. `fh serve --stop` stops it. When 2 `fh run` commands start a daemon at the same time, the lock lets one start, and both use it. The other `fh run` stops the `fh serve` that it started, so that this `fh serve` cannot take the lock later.

## Store

```
<state>/daemon.lock
<state>/daemon.info
<state>/runs/<r>/input.inp                  the read-only copy of the input that the run solves
<state>/runs/<r>/record
<state>/runs/<r>/events.log
<state>/runs/<r>/out/                       protocol v2 run folder
<state>/previews/<id>.inp                   the copy of the input of a preview
<state>/open.html                           the page that sends the browser on to the page of the daemon (mode 0600)
<state>/daemon.log                          the output of a daemon that fh run started (mode 0600)
<state>/examples/<name>/                    the case files of fh run --example
<state>/cases/<name>-<hash>/                the mesh and the input of a case folder of fh run
```

The daemon writes each record to a temporary file and then renames it. Thus a reader never sees a partial file.

## `fh serve`

```
fh serve [--state DIR] [--port N] [--no-open]
fh serve --stop [--state DIR]
```

`fh serve` starts the daemon and runs until a stop signal. It listens on 127.0.0.1 only. No option changes the address.

| Option | Default |
|---|---|
| `--state DIR` | `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy`. |
| `--port N` | 0, a free port. The port is in `daemon.info`. |
| `--no-open` | Off: `fh serve` opens its page in the browser. `FH_NO_OPEN` with a value that is not empty and not `0` does the same as `--no-open`. |
| `--stop` | Stop the daemon that answers for the state folder, and wait until it frees `daemon.lock`. |

The solver is `$FH_SOLVE`, else `fh-solve-ccx` beside `fh`, else `fh-solve-ccx` on `PATH`. It needs CalculiX (`docs/ccx.md`). The wasm viewer is in `$FH_VIEWER`, or in the `viewer` folder beside `fh` (`scripts/install.sh` puts it there). At start, `fh serve` prints the address of the page with the token, `http://127.0.0.1:<port>/?token=<token>`, the viewer folder and the state folder. When the viewer is not built, `fh serve` says so and the page shows the error.

Then `fh serve` opens the page in the browser. The token must not go into the command line of a browser process, because other users of the machine can read a command line. Thus `$BROWSER` (when it is set) and `xdg-open` get the file `<state>/open.html` (mode 0600), which sends the browser on to the address. On macOS, `open` gives the address to the default browser through Launch Services. The exact rules are in [`fh serve`](reference/fh.md#fh-serve).

The page holds no token. Its script reads the token from the page address and sends it in the `Authorization` header to fetch the viewer and the results. Thus only `GET /` takes the token in the query. The page has its own content security policy: only its own script (a new nonce for each answer), the viewer module, wasm, and fetches to its own origin.

SIGINT (Ctrl+C), SIGTERM and SIGHUP stop the daemon in order: it removes `daemon.info`, each active run ends as `interrupted`, and the agent of the chat stops. `fh serve --stop` sends SIGTERM.

| Exit | Meaning |
|---|---|
| 0 | Stopped by a signal. With `--stop`: the daemon stopped, or no daemon answered. |
| 1 | The daemon could not start, for example another daemon uses the state folder (`daemon.running`). With `--stop`: the daemon did not stop in 30 seconds. |
| 2 | Usage error. |

## `fh run`

`fh run` is the client of the contract for a person in a terminal ([`fh run`](reference/fh.md#fh-run)):

1. It finds or starts the daemon (rule 6 of "Which process stays alive").
2. It sends the absolute path of the input file to the preview route, then submits it.
3. It reads the events with `wait_ms=500` and shows them, until the run is final. The state event of a run that fails also gives the reason: `state failed: REASON`. `REASON` is the `message` of the record. Thus `events.log` tells why a run failed.
4. On Ctrl+C, it sends a cancel command and continues to read the events. A second Ctrl+C ends `fh run`. The run continues in the daemon until the cancel ends it.
5. It reads the record. For a run that succeeded, it shows the result folder and `summary.toon`, and opens the page on `&run=R`.

## Tests

`crates/daemon/tests/contract.rs` has the D1 exit checks:

- an authorized CLI passes, and a foreign `Origin` fails,
- a closed browser does not stop the run, and a client that reads the events over many connections gets each event 1 time,
- cancel keeps one record,
- a run solves a copy of its input,
- the event queue and the event log are bounded,
- a stopped daemon leaves `interrupted` records,
- after a failed daemon (SIGKILL), the solver stops, and the next daemon shows the run as `interrupted`.

`crates/cli/tests/run.rs` tests `fh run` and the browser and stop options of `fh serve`: an `.inp` file, a case folder, the example, a second run on the same daemon, the browser open and `--no-open`, a missing file, an input that the solver refuses, a failed run, no daemon possible, Ctrl+C, and a second Ctrl+C.

`crates/daemon/tests/serve.rs` tests `fh serve`: a missing token, a wrong token and a wrong `Origin` each fail, the token with no `Origin` or with the own `Origin` passes, a network address of the machine gets no connection, a second daemon on the same state folder fails, and a stop or SIGTERM stops the daemon and removes `daemon.info`.
