# Concepts of the fh serve page

This is the conceptual model of the page of `fh serve` (`crates/fh/src/daemon/page.html`) and of a report. It has no layout, widgets or colours. `DESIGN.md` section 8 has the look.

Posture: sovereign. The person keeps the page open for a full work session, and the model view is the largest area of each screen.

## Concepts

| Concept | Purpose | State | Actions | Operational principle |
| --- | --- | --- | --- | --- |
| Message | Tell the agent what to build and solve. | The text of each message of the person and of the agent, in sequence. | send, stop | After you send a message, the agent replies in the chat and its tool rows show what it does. |
| Turn | Show that the agent works now. | Busy or idle, the elapsed time, the current tool. | stop | After you stop a turn, the agent and its commands end, and you can send again. |
| Solve | Show each solve that runs now. | The run, the folder, the elapsed time, the solved fraction. | (none: it ends by itself) | After the agent starts a solve and ends its turn, the solve stays in the list until `fh run` ends. |
| Run | Keep the results of 1 solve. | The ID, the source path, the status, the files, the report. | open, search, compare, open file, open report, prepare rerun | After you open a run, the model view shows its mesh and its results. |
| Model view | Show the field of a run on its mesh. | The camera, the field, the step, the marks, the panel. | orbit, pan, zoom, fit, reset camera, pick field, probe, hide panel | After you pick a field, the colours and the legend change at once. |
| Comparison | Show 2 meshes side by side. | Run A, run B, linked cameras. | compare, link cameras, done | After you choose A and B, both models move together while the cameras stay linked. |

## Objects (ORCA)

| Object | Relationships | Calls-to-action (frequency) | Attributes |
| --- | --- | --- | --- |
| Message | 1 chat has many messages. 1 agent message has 0 to many tool rows. | send (many times a day), stop (rarely) | text, author, tool rows, failure |
| Run | 1 state folder has many runs. 1 run has many files and 0 or 1 report. | open (many times a day), search (some times a day), compare (rarely), prepare rerun (rarely) | ID, source path, status, open or not |
| File | 1 run has many files. | open (some times a day) | name |
| Report | 1 run has 0 or 1 report. | open (some times a day) | path |
| Model | 1 open run has 1 model, or 2 in a comparison. | orbit, pan, zoom (many times a day), fit, reset, field, probe (some times a day) | mesh, field, legend, marks |

## Synchronisations

- When the agent writes a new result while the model is out of view, the page shows "New result". A tap on it opens the run.
- When a run opens, the Runs list closes and the address of the page keeps the run.
- When Compare runs is on, the next opened runs go to A and B.

## Object-action matrix

The count is the same before and after this change: this change is a look change and keeps each control in its place.

| Object | send | stop | open | search | compare | open file | open report | rerun |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Message | composer | composer | n/a | n/a | n/a | n/a | n/a | n/a |
| Run | n/a | n/a | row in Runs | Runs | Runs | Files | toolbar | chat (after a stop) |
| File | n/a | n/a | Files | (empty) | n/a | n/a | n/a | n/a |
| Report | n/a | n/a | toolbar | n/a | n/a | n/a | n/a | n/a |

Faults: 2.

- Expected empty cell: you can search the runs but not the files of a run.
- Displaced action: Prepare rerun is in the chat, not on the run row.
