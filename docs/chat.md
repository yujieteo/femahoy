# Chat with the agent

This page describes the chat of `fh serve` (roadmap item M4, DESIGN.md section 5.1) as it is built.
The code is in `crates/daemon/src/chat.rs` (the agent process and the routes) and `crates/viewer/src/chat.rs` (the panel).

## Summary

- You write a request in the chat of the page, for example "Build and solve a steel cantilever of 100 x 10 x 10 mm with 1 MPa on the top face".
- The daemon runs 1 agent: Claude Code in headless mode, with your own Claude Code login. femahoy has no agent loop of its own.
- The agent makes the CAD with build123d, then meshes and solves with `fh` commands. The chat shows each action: the text of the agent, each command with its result, each file that it writes, and the end with the wall time and the cost.
- Before each solve, the viewer of the page shows the model with each support and each load. When a support or a load is not clear in the request, the agent shows the candidates and asks you. It solves only after your answer.
- When a run succeeds, the viewer of the page shows its results. A link in the chat shows them again.
- The browser holds no API key. The agent runs in the daemon.

## Start

```sh
fh serve
```

Then write a request in the `Chat` panel on the left, and click `Send` (or press Ctrl+Enter).
`Stop` stops the agent and the commands that it started.
The next request continues the same session, so the agent knows the earlier steps.

The agent needs:

- `claude` (Claude Code) on `PATH`, with a login. Set `FH_AGENT` to use another program with the same arguments and event stream.
- gmsh for `fh mesh`.
- `fh-python` on `PATH` for the CAD script. `scripts/install.sh` installs it beside `fh`, with build123d.

## What the agent does

The daemon starts the agent in the folder `STATE/chat` with:

```text
claude -p --output-format stream-json --verbose --append-system-prompt <agent.md>
  --allowedTools Bash(fh *),Bash(fh-python *),Read,Write,Edit,Glob,Grep
  [--resume SESSION]
```

The request goes on stdin. The instructions are in `crates/daemon/src/agent.md`: write `part.py` and `case.toml`, then `fh info`, then `fh run .`, which runs `fh mesh`, `fh model`, the preview and the solve on the daemon of the page.
The agent can use only the `fh` commands, `fh-python` and the file tools. Claude Code refuses each other tool in headless mode.
The environment of the agent has `FH_STATE` (the state folder of the daemon, so that `fh run` uses this daemon), `FH_NO_OPEN=1` and the folder of `fh` first on `PATH`.

## The model before the solve

A wrong support face or a wrong load direction gives a result that looks correct. Thus the page shows the supports and the loads before each solve.

1. `fh run` makes the input file with `fh mesh` and `fh model`, then sends it to `POST /api/v1/projects/P/preview`.
2. The daemon keeps the input as a revision, and adds a `show` entry to the chat.
3. The page opens the entry: it gets the revision and shows the model.
4. `fh run` then solves the same revision. With `--preview`, it stops before the solve.

The viewer shows:

- the faces of each support in a blue, teal, violet or green colour, and the faces of each load in a red, orange, magenta or yellow colour. The other faces are grey.
- 1 arrow for each load, in the direction of the total force of the load. The arrow of a push ends on the face. The arrow of a pull or a shear starts on the face.
- a label for each region: the region name of `case.toml` and what acts on it. The label of a load is at the tail of its arrow.
- a list with each region: its label, its actions (for example `U1 to U3 = 0` or `pressure 1 MPa`), its number of nodes, its area and its total force.

| Region | Label |
|---|---|
| A support | the fixed degrees of freedom, for example `FIXED: ux uy uz` or `PIN: uy uz`. A set value shows as `ux = 0.1 mm`. |
| A force on nodes (`cload`) | the total force and its direction, for example `P1: 2700 N, -y`. A direction that is not on an axis shows as a unit vector. |
| A pressure (`dsload`) | the pressure, for example `TOP: 1 MPa pressure`. The arrow shows the direction. |
| A temperature (a CalculiX case) | `ALL: temperature 0 to 100`, at the middle of the part. It has no arrow. |

The page reads the solver input, not `case.toml`. Thus it shows the faces that the solver gets.
A region that has no face shows a warning in the list.

The results of a run show the same labels and arrows, and the outline of the faces of each region. The page gets the input from the revision in the run record. The labels move with the deformed shape.
To hide the labels and the arrows, clear `Show them on the model` under `Supports and loads` in the results panel, or `Show the labels and the arrows` in the panel before the solve.

The agent has 1 rule for the supports and the loads (`crates/daemon/src/agent.md`):

- When the request tells the fixed faces, the load face and the load direction, the agent shows the model and solves with no question.
- Else the agent makes 1 case folder for each candidate and runs `fh run CANDIDATE --preview` for each. The page shows each candidate. The agent asks you which candidate is correct, and stops. After your answer, it solves that candidate.

A link `Supports and loads of NAME before the solve (REVISION)` in the chat shows a candidate again.

## Routes

The routes have the token and `Origin` rules of the other routes of `fh serve` (`docs/run-contract.md`).

| Method and path | Body | Answer |
|---|---|---|
| `GET /api/v1/chat?since=N` | | `state` (`running` or `idle`), `next`; one item `<seq> <kind> <text>` for each entry after `N` |
| `POST /api/v1/chat` | `text` | `state=running`. `chat.empty` for an empty request, 409 `chat.busy` while the agent works |
| `POST /api/v1/chat/stop` | | `state` |
| `POST /api/v1/projects/P/preview` | `input`, `title` | `revision`. The chat gets the entry `show`. The errors are those of a submit (`docs/run-contract.md`) |
| `GET /api/v1/projects/P/revisions/REV` | | the bytes of the input file of the revision. 400 `revision.invalid`, 404 `revision.unknown` |

| Kind | Text |
|---|---|
| `you` | the request |
| `note` | the model and the folder of the agent, the stderr of the agent, a stop |
| `say` | a text of the agent |
| `cmd` | a shell command of the agent, for example `fh run . --size 2.5` |
| `tool` | another tool and its file, for example `Write /.../part.py` |
| `out`, `fail` | the result of a tool, or its error (at most 4000 characters) |
| `show` | `<project> <revision> <title>`: a model before the solve, with its supports and loads |
| `open` | `<project> <run>`: a run that succeeded during the chat |
| `end` | `done in S s, N turns, cost C USD`, from the `result` event of the stream |

In the text, `\n` is a line break and `\\` is a backslash. The token of the daemon shows as `<token>`.
The daemon keeps the last 4000 entries in memory. A new daemon starts with an empty chat.

## Tests

`crates/daemon/tests/chat.rs` runs a fake agent that prints a recorded event stream (`crates/daemon/tests/data/agent-*.jsonl`). No test calls a real LLM.
The test does a preview before the solve. It checks that the `show` entry comes before the `open` entry, and that the run solves the shown revision.
`crates/daemon/tests/viewer.rs` checks the revision route: the bytes, the token and the `Origin` rules, and each refusal.
`crates/viewer/src/preview.rs` checks the preview data: the faces and the outline of each region, the area, the total force, the direction of each arrow and each label. It also checks that the labels of a run move with its results.
`crates/viewer/src/chat.rs` tests the panel with a fake daemon.

## M4: the first slice and the open rest

The first slice of M4 is the M4 exit test: from 1 chat request, 1 agent builds and solves a cantilever model, and every action shows as an `fh` command.

| Item | Status |
|---|---|
| 1 agent in headless mode in the chat, with the login of the user (connection path 1) | Built |
| Each action of the agent in the chat, and the results in the viewer | Built |
| The supports and the loads in the viewer before each solve, and a question when they are not clear | Built |
| The labels of the supports and the loads on the results | Built |
| Fake agent with a recorded event stream in the tests | Built |
| Connection path 2: the API key of the user in the environment of the agent | Open |
| More agents in panes, and the tree of the agents | Open |
| `femahoy.toml` (the harness and the model of each agent) | Open |
| `fh init` installs the skills | Open |
| Codex as the agent (`codex exec --json`) | Open |
