# Run contract (D1)

The run contract is one contract for runs. The `fh` CLI and the UI both use it. The code is in `crates/daemon`. `fh serve` runs the daemon (see [`fh serve`](#fh-serve)).

The solve path does not change. A run starts the protocol v2 solver (`fh-solve`, `docs/solver-protocol.md`) on a fixed copy of the input file.

## Words

| Word | Meaning |
|---|---|
| Project | A root folder. The daemon gives each root one project ID, `p-` and 16 hex digits. |
| Revision | A read-only copy of the input file at submit time. Its ID is `rev-` and the FNV-1a hash of the bytes. This is the same hash as `source_hash` in the `fh.run` summary. |
| Run | One solve of one revision. Its ID is `run-000001`, `run-000002` and so on, in each project. A run has one record. |
| Request ID | An ID that the client makes for each submit and each cancel. A retry keeps the same ID. |
| Acknowledgement | The answer to a submit or a cancel. The daemon keeps it on disk. |

## Run states

```
queued -> running -> succeeded | failed | cancelling | interrupted
queued -> cancelled | interrupted
cancelling -> cancelled | succeeded | failed
```

`succeeded`, `failed`, `cancelled` and `interrupted` are final. A final state never changes.

## Commands and outcomes

| Command | Outcome | Meaning |
|---|---|---|
| submit | `created` | The daemon made a new run. |
| submit | `existing` | A run of the same revision was still active in the project. The daemon made no new run. |
| cancel | `cancel-requested` | A queued run ended at once, or the solver of a running run got `cancel`. |
| cancel | `already-final` | The run had ended. Nothing changed. |
| submit or cancel | `replay` | The daemon did this request ID before. It sends the same run with its state now. |

A request ID that a client uses again for another command fails with `request.reused` (409).

## Access

The daemon listens on 127.0.0.1 only. It makes a random token at each start and writes the port and the token to `<state>/daemon.info` (mode 0600).

1. The `Host` header must be `127.0.0.1:<port>`.
2. A request with an `Origin` header must come from `http://127.0.0.1:<port>`, the page of the daemon. Any other `Origin` fails with 403 `origin.refused`.
3. Every request must carry `Authorization: Bearer <token>`. A client with no `Origin`, such as the CLI, needs only the token. Only `GET /`, the page of the daemon, can also take the token as `?token=<token>`, because a browser that opens a link sends no header.

Each answer has `Referrer-Policy: no-referrer`, so the page never sends its address with the token to another site.

## Messages and routes

A message is UTF-8 text with one `key=value` line for each field. An empty line ends the fields. Item lines can follow, for example the events of a run. Each message has `contract=1`. An error has the fields `error` and `message`.

| Method and path | Body | Answer |
|---|---|---|
| `GET /` | | the page of the daemon (HTML). The wasm viewer replaces it later. |
| `GET /api/v1/status` | | `instance`, `pid` |
| `POST /api/v1/projects` | `root` | `project`, `root` |
| `GET /api/v1/projects/<p>/runs` | | one item `<run> <state>` for each run |
| `POST /api/v1/projects/<p>/runs` | `request`, `input` | acknowledgement |
| `GET /api/v1/projects/<p>/runs/<r>` | | run record |
| `POST /api/v1/projects/<p>/runs/<r>/cancel` | `request` | acknowledgement |
| `GET /api/v1/projects/<p>/runs/<r>/events?since=N&wait_ms=T` | | events after `N` |

The `input` of a submit is a path in the project root. A path outside the root fails with `input.outside`.

## Limits

| Limit | Value |
|---|---|
| Runs in the queue | 64. A submit after that fails with `queue.full` (503). |
| Events of a run in memory | 1024. A client that reads too late gets the count in `dropped`. |
| Event text | 1024 bytes. A longer text is cut. |
| Event log of a run | 1 MiB. Then the log gets one `log-full` line. State events are always written. |
| Events in one answer | 256 |
| Wait of an events request | 30 seconds |
| Acknowledgements on disk | 4096. At start, the daemon removes the oldest. |
| Connections at the same time | 32 |

The protocol v2 runner writes the full solver output to `out/stdout.txt`. The contract does not limit that file.

## Which process stays alive

The daemon owns every run. A browser, the native UI and the CLI are only clients.

1. A client that closes or disconnects stops nothing. The run continues. The client reads the same record when it connects again.
2. Only a cancel command stops a run on purpose.
3. The solver never lives longer than the daemon. A daemon that stops cancels its runs, and they end as `interrupted`. A daemon that fails closes the stdin of the solver, and protocol v2 then stops the solver.
4. At the next start, the daemon marks each run that was still active as `interrupted`. A run with a pending cancel becomes `cancelled`. The daemon never starts such a run again by itself. The run folder keeps the restart file of the solver.
5. One daemon at a time uses a state folder. The lock file `daemon.lock` makes this sure.

## Store

```
<state>/daemon.lock
<state>/daemon.info
<state>/acks/<request>
<state>/projects/<p>/root
<state>/projects/<p>/revisions/<revision>/input.inp
<state>/projects/<p>/runs/<r>/record
<state>/projects/<p>/runs/<r>/events.log
<state>/projects/<p>/runs/<r>/out/          protocol v2 run folder
```

The daemon writes each record and each acknowledgement to a temporary file and then renames it. Thus a reader never sees a partial file.

## `fh serve`

```
fh serve [--state DIR] [--port N]
```

`fh serve` starts the daemon and runs until a stop signal. It listens on 127.0.0.1 only. No option changes the address.

| Option | Default |
|---|---|
| `--state DIR` | `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy`. On Windows, `%LOCALAPPDATA%\femahoy`. |
| `--port N` | 0, a free port. The port is in `daemon.info`. |

The solver is `$FH_SOLVE`, or `fh-solve` beside `fh`. At start, `fh serve` prints the address of the page with the token, `http://127.0.0.1:<port>/?token=<token>`, and the state folder.

SIGINT (Ctrl+C), SIGTERM and SIGHUP stop the daemon in order: it removes `daemon.info`, and each active run ends as `interrupted`. On Windows, Ctrl+C ends the process. That is the path of a failed daemon (see "Which process stays alive").

| Exit | Meaning |
|---|---|
| 0 | Stopped by a signal. |
| 1 | The daemon could not start, for example another daemon uses the state folder (`daemon.running`). |
| 2 | Usage error. |

## Tests

`crates/daemon/tests/contract.rs` has the D1 exit checks:

- an authorized CLI passes, and a foreign `Origin` fails,
- a duplicate submission keeps one run,
- a CLI that connects again gets the same run,
- a closed browser does not stop the run,
- cancel keeps one record,
- a stopped daemon and a failed daemon (SIGKILL) each leave one `interrupted` record.

`crates/daemon/tests/serve.rs` tests `fh serve`: a missing token, a wrong token and a wrong `Origin` each fail, the token with no `Origin` or with the own `Origin` passes, a network address of the machine gets no connection, a second daemon on the same state folder fails, and a stop or SIGTERM stops the daemon and removes `daemon.info`.
