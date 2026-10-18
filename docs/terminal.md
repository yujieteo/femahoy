# Terminal sidebar and pane backend

This page describes the terminal sidebar of `fh serve` (DESIGN.md sections 3 and 4.3) as it is built.
The code is in `crates/daemon` (`pty.rs`, `panes.rs`, `ws.rs`, `screen.rs`, `http.rs`) and in `crates/viewer` (`terminal.rs`, `sidebar.rs`).

## Summary

- The terminal sidebar is a real shell on this machine. It shows beside the 3D view of the viewer page.
- The daemon owns the PTY of each terminal. A terminal continues to run when the browser closes. A new browser gets the full screen again.
- The daemon owns the pane backend: plain PTYs of the daemon.
- Each terminal and pane route has the same token and `Origin` rules as the other routes of `fh serve`. The daemon listens on 127.0.0.1 only.

## Start

```sh
fh serve [--state DIR] [--port N]
```

`fh serve` prints one line for the backend:

```text
fh serve: panes pty (plain PTY panes owned by the daemon)
```

The sidebar shell is `$SHELL -l`, else `/bin/sh`.

## Access

A terminal is a real shell, so the access rules are its only way in.
The rules are the rules of `crates/daemon/src/http.rs`:

1. The `Host` header must be `127.0.0.1:<port>`.
2. A request with an `Origin` header must come from `http://127.0.0.1:<port>`. Any other `Origin`, also `null` and `http://localhost`, fails with 403 `origin.refused`.
3. Each request must have `Authorization: Bearer <token>`. A missing token fails with 401 `token.missing`, and a wrong token fails with 401 `token.invalid`.
4. A browser cannot set a header on a WebSocket. Thus the WebSocket route also takes the token as the subprotocol item `fh.token.<token>`. No other route takes it there.
5. Only `GET /` takes the token in the query. A terminal route never does.

`crates/daemon/tests/terminal.rs` tests a missing token, a wrong token, a wrong `Origin` and the token in the wrong place on each terminal and pane route, the WebSocket route included.

## The sidebar in the page

The page of `fh serve` shows the sidebar on the right of the 3D view and the results panel.

- The `Terminal` switch in the status bar shows or hides the sidebar. Drag the left edge of the sidebar to change its width.
- `+ Shell` opens a new sidebar shell.
- Each shell terminal of the daemon has a tab. The label is the terminal ID, then the title that the program set (or the kind), then the exit code when the program ended.
- `Close` stops the program of the tab and removes the terminal from the daemon. When you close the browser page, the programs continue to run.
- When the WebSocket closes, the tab shows the reason and `Connect again`.
- The `Panes` section shows the backend, each pane, and a form for a new pane (a title and an optional command). `Show` opens a pane in a tab.
- The page reads the terminal list and the panes every 5 seconds. Thus a terminal that another page opened, or a terminal that ended, shows without a reload.

Keys and mouse:

- Click the terminal to give it the keys. A frame shows which terminal has the keys.
- Tab, the arrow keys and Esc go to the program, not to the page.
- Ctrl with a letter sends the control character. Alt with a key sends Esc before the key, but not on macOS, where Option types characters.
- Drag with the mouse to select text. On macOS, Cmd+C copies the selection, and Ctrl+C sends the interrupt (`0x03`). On the other systems, Ctrl+C with a selection copies it, clears the selection and sends nothing. Ctrl+C with no selection sends `0x03` and copies nothing. Ctrl+X only sends `0x18`: a terminal never cuts.
- The paste key of the browser pastes (Cmd+V on macOS, Ctrl+V or Ctrl+Shift+V on the other systems). On macOS, Ctrl+V sends `0x16`. In bracketed paste mode, the program gets the paste as one block, and the page removes each Esc character from the pasted text so that the text cannot end the block early.
- When a program asks for the mouse, the clicks, the drags and the wheel go to the program. Hold Shift to select text.
- The wheel scrolls the scrollback of the daemon on the normal screen. On the alternate screen, the wheel sends the arrow keys when the program has alternate scroll, and nothing else. A program with mouse mode gets mouse reports. A box at the top right shows how many lines are above the bottom. A key press goes back to the bottom.

Limits of the page:

- The browser keeps some keys, for example Cmd+W, Cmd+T and Cmd+Q on macOS and Ctrl+W on the other systems. The program does not get them.
- The page has only the default fonts of egui. A character that they do not have, for example CJK text or some symbols, shows as a box. The width of the cell is correct.

## Terminals

A terminal is one program in one PTY of the daemon.

| Kind | Program |
|---|---|
| `shell` | the sidebar shell |
| `pane` | the shell of a PTY pane |

Each program gets these environment variables: `TERM=xterm-256color`, `COLORTERM=truecolor` and `TERM_PROGRAM=femahoy`.

Limits:

| Item | Limit |
|---|---|
| Terminals at the same time | 16. When the list is full, a new terminal removes the oldest ended one, else fails with 503 `term.limit`. |
| Terminal WebSockets at the same time | 16 (503 `term.busy`) |
| Scrollback | 10000 lines |
| Size | 2 to 500 columns, 1 to 200 rows |

States: `running`, `exited` (the program ended, the record stays) and `closed`.
A close sends SIGHUP to the process group of the program. If the program continues, the daemon sends SIGKILL after 1 second. SIGHUP and SIGKILL go to the process group of the program only. A job in another process group that ignores SIGHUP continues, as in other terminal programs.
When the daemon stops, it closes each terminal.

## Pane backend

All code that shows or drives panes uses the `PaneBackend` trait (`crates/daemon/src/panes.rs`).

| Operation | pty backend |
|---|---|
| open | a new `pane` terminal. The command is typed with Enter. |
| list, get | the `pane` terminals |
| read | the last lines of the terminal text |
| send | the text bytes, then the key bytes |
| close | close the terminal |

The pane ID is the terminal ID, so the sidebar shows the pane as a terminal.

### Keys

`POST /api/v1/panes/<pane>/send` takes `keys` as names with a space between them:
`enter`, `tab`, `esc`, `backspace`, `up`, `down`, `right`, `left`, `ctrl+c` and `ctrl+d`.
An unknown name fails with 400 `pane.key`.

### Pane checks

| Field | Rule | Error |
|---|---|---|
| `title` | 1 to 64 characters, no control character | 400 `pane.title` |
| `cwd` | an absolute path to a folder. The default is the folder of the daemon. | 400 `pane.cwd` |
| `command` | one line of at most 4096 bytes | 400 `pane.command` |
| pane ID | 1 to 64 letters, digits, `:`, `_` or `-`, a letter or a digit first | 404 `pane.unknown` |

## Routes

| Method and path | Body | Answer |
|---|---|---|
| `GET /api/v1/terminals` | | `panes` (the backend); one item `<t> <kind> <state> <title>` for each terminal |
| `POST /api/v1/terminals` | `kind` (`shell`) | terminal record |
| `GET /api/v1/terminals/<t>` | | terminal record |
| `GET /api/v1/terminals/<t>/text?lines=N` | | one item for each line of text (default 50) |
| `POST /api/v1/terminals/<t>/close` | | terminal record |
| `GET /api/v1/terminals/<t>/ws` | | WebSocket upgrade, subprotocol `fh.term.v1` |
| `GET /api/v1/panes` | | `backend`, `detail`; one item `<pane> <state> <t or -> <title>` for each pane |
| `POST /api/v1/panes` | `title`, optional `cwd` and `command` | pane record |
| `GET /api/v1/panes/<pane>` | | pane record |
| `GET /api/v1/panes/<pane>/text?lines=N` | | one item for each line of text (default 50) |
| `POST /api/v1/panes/<pane>/send` | optional `text` and `keys` | |
| `POST /api/v1/panes/<pane>/close` | | |

A terminal record has `terminal`, `kind`, `state`, `title` and, after the end, `exit`.
A pane record has `pane`, `state`, `terminal` (or `-`) and `title`.
The body and the answer use the text form of the run contract (`docs/run-contract.md`).

## WebSocket protocol `fh.term.v1`

The client asks for the subprotocols `fh.term.v1` and `fh.token.<token>`.
The daemon answers with `fh.term.v1` only. It never sends the token back.
All messages are binary. All numbers are big-endian.
The daemon sends a ping each 10 seconds. A client that sends nothing for 30 seconds has gone.

### Messages from the client

| Type | Name | Payload |
|---|---|---|
| 1 | input | the bytes to write to the PTY, for example the UTF-8 text of the keys |
| 2 | resize | u16 columns, u16 rows |
| 3 | scroll | i32 lines: more than 0 moves the view up into the scrollback, less than 0 moves it down, 0 goes to the bottom |

An input also moves the view to the bottom.
A message of an unknown type closes the WebSocket with 1003.

### Messages from the daemon

| Type | Name | Payload |
|---|---|---|
| 1 | screen | the screen head, then the changed rows |
| 2 | title | the UTF-8 title that the program set (at most 120 characters) |
| 3 | exit | i32 exit code. A program that a signal stopped has the negative signal number. `-1` is not known. |

After the exit message, the daemon closes the WebSocket with 1000.

### Screen message

| Bytes | Field |
|---|---|
| 1 | type 1 |
| 2 | columns |
| 2 | rows |
| 2 | cursor column |
| 2 | cursor row |
| 1 | cursor shape: 0 hidden, 1 block, 2 underline, 3 beam, 4 hollow |
| 4 | modes (bits below) |
| 4 | default foreground, `0x00RRGGBB` |
| 4 | default background, `0x00RRGGBB` |
| 4 | scrolled: the lines that the view is up in the scrollback |
| 4 | history: the lines in the scrollback |
| 2 | the number of rows that follow |

Each row:

| Bytes | Field |
|---|---|
| 2 | row index, 0 at the top |
| 2 | the number of spans |

Each span: cells with the same colours and style.

| Bytes | Field |
|---|---|
| 2 | first column |
| 2 | columns |
| 4 | foreground, `0x00RRGGBB` |
| 4 | background, `0x00RRGGBB` |
| 1 | style (bits below) |
| 2 | text length in bytes |
| n | UTF-8 text |

A cell with no span is a space with the default colours.
A cell that has zero-width marks (for example a combining accent) is its own span, with the base character and at most 16 marks in the text. A span of one column, or with the wide style, shows its whole text as one cluster in its first cell. Each other span has one character for each column.
A wide character in the last column has no room for its second cell, so the daemon sends a space of one column.
The first message to a client has all rows. Each later message has only the rows that changed. The daemon sends no message when nothing changed.
A change of the size sends all rows again.

Mode bits: 1 application cursor keys, 2 application keypad, 4 bracketed paste, 8 mouse clicks, 16 mouse drag, 32 mouse motion, 64 SGR mouse, 128 UTF-8 mouse, 256 alternate screen, 512 alternate scroll, 1024 focus in and out, 2048 line feed and new line.

Style bits: 1 bold, 2 italic, 4 underline, 8 strikeout, 16 wide (one wide character over 2 columns).

The daemon applies the inverse, dim and hidden attributes to the colours before it sends a span.

`fixtures/terminal/screen-v1.hex` has one screen message. The encoder of the daemon and the decoder of the viewer test against it.

## Errors

| Code | Status | Cause |
|---|---|---|
| `term.unknown` | 404 | no terminal with this ID |
| `term.kind` | 400 | a kind that is not `shell` |
| `term.program` | 409 | the program is not on `PATH` |
| `term.spawn` | 500 | the PTY or the program did not start |
| `term.ended` | 409 | input to a terminal that ended |
| `term.limit` | 503 | 16 terminals run |
| `term.busy` | 503 | 16 terminal WebSockets are open |
| `ws.invalid` | 400 | a WebSocket request with a bad `Upgrade`, `Connection`, version, key or subprotocol |
| `pane.unknown` | 404 | no pane with this ID in the backend |
| `pane.title`, `pane.cwd`, `pane.command`, `pane.key` | 400 | a bad pane field |

## Tests

| Test | What it shows |
|---|---|
| `crates/daemon/tests/terminal.rs` `every_terminal_and_pane_route_keeps_the_access_rules` | the access rules on each terminal and pane route |
| `a_shell_runs_over_the_websocket_and_outlives_its_browser` | input, resize, output, a new browser, the exit code |
| `pty_panes_run_commands_and_take_keys` | the pty backend and its errors |
| `crates/daemon/src/pty.rs`, `screen.rs`, `ws.rs` unit tests | colours, wide text, scrollback, close, the frame rules, the fixture |
| `crates/viewer/src/terminal.rs` unit tests | the decoder against the fixture, a bad message that changes nothing, the client messages, the keys, the paste, the mouse reports, the selection |
| `crates/viewer/src/sidebar.rs` unit tests | with a fake daemon: the tabs follow the terminal list, open, show, close, the panes form, the error line, the tab label |
