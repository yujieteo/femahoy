# Chat with the agent

This page describes the chat of `fh serve` (roadmap item M4, DESIGN.md section 5.1) as it is built.
The code is in `crates/daemon/src/chat.rs` (the agent process and the routes) and `crates/daemon/src/page.html` (the chat panel of the page, in HTML and plain JS).

## Summary

- You write a request in the chat of the page, for example "Build and solve a steel cantilever of 100 x 10 x 10 mm with 1 MPa on the top face".
- The daemon runs 1 agent: Claude Code in headless mode, with your own Claude Code login. femahoy has no agent loop of its own.
- The agent makes the CAD with build123d, then meshes and solves with `fh` commands. The chat shows each action: the text of the agent, each command with its result, each file that it writes, and the end with the wall time and the cost.
- Before each solve, the viewer of the page shows the model with each support and each load. When a support or a load is not clear in the request, the agent shows the candidates and asks you. It solves only after your answer.
- You can send a message while the agent works. The agent reads it at its next step, so you can ask for the status or change the plan.
- While the agent works, a status above the text box shows the time, the last command or file, the run that solves now with its progress, and each failed solve.
- When a run succeeds, the viewer of the page shows its results. A button in the chat shows them again. When a run fails, the chat shows its error in red.
- The browser holds no API key. The agent runs in the daemon.

## Start

```sh
fh serve
```

Then write a request in the chat on the left of the page, and click `Send` (or press Ctrl+Enter).
The text box stays available while the agent works. A message goes to the agent at once, and the agent reads it at its next step: after the command that runs now.
`Stop` stops the agent and the commands that it started.
The next request continues the same session, so the agent knows the earlier steps.

The agent needs:

- `claude` (Claude Code) on `PATH`, with a login. Set `FH_AGENT` to use another program with the same arguments and event stream.
- gmsh for `fh mesh`.
- `fh-python` on `PATH` for the CAD script. `scripts/install.sh` installs it beside `fh`, with build123d.

## What the agent does

The daemon starts the agent in the folder `STATE/chat` with:

```text
claude -p --input-format stream-json --output-format stream-json --verbose
  --replay-user-messages --append-system-prompt <agent.md>
  --allowedTools Bash(fh *),Bash(fh-python *),Read,Write,Edit,Glob,Grep
  [--resume SESSION]
```

The agent process stays alive between the messages. Each message goes on stdin as 1 line:

```json
{"type":"user","message":{"role":"user","content":"TEXT"},"parent_tool_use_id":null}
```

The agent replays each message when it reads it (`--replay-user-messages`). Thus the daemon knows that the agent works while it has a message that it did not read, or a turn with no `result` event.
After a stop, or when the agent ends, the next message starts a new agent with `--resume`. The instructions are in `crates/daemon/src/agent.md`: write `part.py` and `case.toml`, then `fh run .`, which runs `fh mesh`, `fh model`, the preview and the solve on the daemon of the page.
The agent can use only the `fh` commands, `fh-python` and the file tools. Claude Code refuses each other tool in headless mode.
The environment of the agent has `FH_STATE` (the state folder of the daemon, so that `fh run` uses this daemon), `FH_NO_OPEN=1` and the folder of `fh` first on `PATH`.
It also has `BASH_DEFAULT_TIMEOUT_MS` and `BASH_MAX_TIMEOUT_MS` set to 86400000 (24 hours). Claude Code stops a Bash command after 2 minutes by default, and a solve of a large model can take hours.

## The model before the solve

A wrong support face or a wrong load direction gives a result that looks correct. Thus the page shows the supports and the loads before each solve.

1. `fh run` makes the input file with `fh mesh` and `fh model`, then sends its path to `POST /api/v1/previews`.
2. The daemon copies the input file as a preview, and adds a `show` entry to the chat.
3. The page opens the entry: it gets the copy and shows the model.
4. `fh run` then solves the same file. With `--preview`, it stops before the solve.

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

The results of a run show the same labels and arrows, and the outline of the faces of each region. The page gets the copy of the input that the run solved. The labels move with the deformed shape.
To hide the labels and the arrows, clear `Show them on the model` under `Supports and loads` in the results panel, or `Show the labels and the arrows` in the panel before the solve.

The agent has 1 rule for the supports and the loads (`crates/daemon/src/agent.md`):

- When the request tells the fixed faces, the load face and the load direction, the agent shows the model and solves with no question.
- Else the agent makes 1 case folder for each candidate and runs `fh run CANDIDATE --preview` for each. The page shows each candidate. The agent asks you which candidate is correct, and stops. After your answer, it solves that candidate.

A button `Show the supports and loads of NAME` in the chat shows a candidate again.

## Routes

The routes have the token and `Origin` rules of the other routes of `fh serve` (`docs/run-contract.md`).

| Method and path | Body | Answer |
|---|---|---|
| `GET /api/v1/chat?since=N` | | `state` (`running` or `idle`), `next`; one item `<seq> <kind> <text>` for each entry after `N`. While the agent works, also `started_ms` and `solving` |
| `POST /api/v1/chat` | `text` | `state=running`: the message goes to the agent, also while it works. `chat.empty` for an empty message, 502 `chat.agent` when the agent cannot start or take the message |
| `POST /api/v1/chat/stop` | | `state=idle` |
| `POST /api/v1/previews` | `input`, `title` | `preview`. The chat gets the entry `show`. The errors are those of a submit (`docs/run-contract.md`) |
| `GET /api/v1/previews/ID` | | the bytes of the copy of the input of the preview. 404 `preview.unknown` |

`started_ms` is the time when the agent started to work, in ms since the Unix epoch. `solving` is the first run of the daemon that is not final: `<run> <state>` and its last `progress` event, for example `run-000001 running progress step=1/1 phase=solve fraction=0.5`.

| Kind | Text |
|---|---|
| `you` | the request |
| `note` | the model and the folder of the agent, the stderr of the agent, a stop |
| `say` | a text of the agent |
| `cmd` | a shell command of the agent, for example `fh run . --size 2.5` |
| `tool` | another tool and its file, for example `Write /.../part.py` |
| `out`, `fail` | the result of a tool, or its error (at most 4000 characters) |
| `show` | `<preview> <title>`: a model before the solve, with its supports and loads |
| `open` | `<run>`: a run that succeeded during the work of the agent |
| `failed` | `<run> <error>`: a run that failed during the work of the agent |
| `error` | an error of the chat: the agent cannot start or take a message, or it ended before the end of its work |
| `end` | `done in S s, N turns, cost C USD`, from the `result` event of the stream |

In the text, `\n` is a line break and `\\` is a backslash. The token of the daemon shows as `<token>`.
The daemon keeps the last 4000 entries in memory. A new daemon starts with an empty chat.

## Tests

`crates/daemon/tests/chat.rs` runs a fake agent that prints a recorded event stream (`crates/daemon/tests/data/agent-*.jsonl`). No test calls a real LLM.
The test does a preview before the solve. It checks that the `show` entry comes before the `open` entry.
The fake takes a second message during its work, and the test solves a run that succeeds and a run that fails during the work. Then it checks the entries, the `solving` and `started_ms` fields, a stop, a new agent with `--resume`, and an agent that ends during its work.
`crates/daemon/tests/viewer.rs` checks the preview route: the bytes, the token and the `Origin` rules, and each refusal. It also checks the layout of the page: the chat on the left of the model view, and no file from the network.
`crates/viewer/src/preview.rs` checks the preview data: the faces and the outline of each region, the area, the total force, the direction of each arrow and each label. It also checks that the labels of a run move with its results.

## M4: the first slice

The first slice of M4 is the M4 exit test: from 1 chat request, 1 agent builds and solves a cantilever model, and every action shows as an `fh` command.

| Item | Status |
|---|---|
| 1 agent in headless mode in the chat, with the login of the user | Built |
| Each action of the agent in the chat, and the results in the viewer | Built |
| The supports and the loads in the viewer before each solve, and a question when they are not clear | Built |
| The labels of the supports and the loads on the results | Built |
| A message to the agent during its work, and the status of the work | Built |
| Fake agent with a recorded event stream in the tests | Built |
