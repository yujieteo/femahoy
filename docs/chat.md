# Chat with the agent

This page describes the chat of `fh serve` (roadmap item M4, DESIGN.md section 5.1) as it is built.
The code is in `crates/daemon/src/chat.rs` (the agent process and the routes) and `crates/viewer/src/chat.rs` (the panel).

## Summary

- You write a request in the chat of the page, for example "Build and solve a steel cantilever of 100 x 10 x 10 mm with 1 MPa on the top face".
- The daemon runs 1 agent: Claude Code in headless mode, with your own Claude Code login. femahoy has no agent loop of its own.
- The agent makes the CAD with build123d, then meshes and solves with `fh` commands. The chat shows each action: the text of the agent, each command with its result, each file that it writes, and the end with the wall time and the cost.
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
- Python with build123d, or `uv`, for the CAD script.

## What the agent does

The daemon starts the agent in the folder `STATE/chat` with:

```text
claude -p --output-format stream-json --verbose --append-system-prompt <agent.md>
  --allowedTools Bash(fh *),Bash(python3 *),Bash(uv run *),Read,Write,Edit,Glob,Grep
  [--resume SESSION]
```

The request goes on stdin. The instructions are in `crates/daemon/src/agent.md`: write `part.py` and `case.toml`, then `fh info`, then `fh run .`, which runs `fh mesh`, `fh model` and the solve on the daemon of the page.
The agent can use only the `fh` commands, Python, `uv run` and the file tools. Claude Code refuses each other tool in headless mode.
The environment of the agent has `FH_STATE` (the state folder of the daemon, so that `fh run` uses this daemon), `FH_NO_OPEN=1` and the folder of `fh` first on `PATH`.

## Routes

The routes have the token and `Origin` rules of the other routes of `fh serve` (`docs/run-contract.md`).

| Method and path | Body | Answer |
|---|---|---|
| `GET /api/v1/chat?since=N` | | `state` (`running` or `idle`), `next`; one item `<seq> <kind> <text>` for each entry after `N` |
| `POST /api/v1/chat` | `text` | `state=running`. `chat.empty` for an empty request, 409 `chat.busy` while the agent works |
| `POST /api/v1/chat/stop` | | `state` |

| Kind | Text |
|---|---|
| `you` | the request |
| `note` | the model and the folder of the agent, the stderr of the agent, a stop |
| `say` | a text of the agent |
| `cmd` | a shell command of the agent, for example `fh run . --size 2.5` |
| `tool` | another tool and its file, for example `Write /.../part.py` |
| `out`, `fail` | the result of a tool, or its error (at most 4000 characters) |
| `open` | `<project> <run>`: a run that succeeded during the chat |
| `end` | `done in S s, N turns, cost C USD`, from the `result` event of the stream |

In the text, `\n` is a line break and `\\` is a backslash. The token of the daemon shows as `<token>`.
The daemon keeps the last 4000 entries in memory. A new daemon starts with an empty chat.

## Tests

`crates/daemon/tests/chat.rs` runs a fake agent that prints a recorded event stream (`crates/daemon/tests/data/agent-*.jsonl`). No test calls a real LLM.
`crates/viewer/src/chat.rs` tests the panel with a fake daemon.

## M4: the first slice and the open rest

The first slice of M4 is the M4 exit test: from 1 chat request, 1 agent builds and solves a cantilever model, and every action shows as an `fh` command.

| Item | Status |
|---|---|
| 1 agent in headless mode in the chat, with the login of the user (connection path 1) | Built |
| Each action of the agent in the chat, and the results in the viewer | Built |
| Fake agent with a recorded event stream in the tests | Built |
| Connection path 2: the API key of the user in the environment of the agent | Open |
| More agents in panes, and the tree of the agents | Open |
| `femahoy.toml` (the harness and the model of each agent) | Open |
| `fh init` installs the skills | Open |
| Codex as the agent (`codex exec --json`) | Open |
