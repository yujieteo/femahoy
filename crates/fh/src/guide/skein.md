# Skein models: a typed simulation language with checked units

The person can ask for a Skein model, or for a model in words that Skein can solve. If Skein is on the computer, write the model in the Skein language and show its source in the chat. Then solve it with `fh run`.

1. Learn the language: run `skein help --llm`. It prints the grammar, the keywords, the standard library and one example.
2. Write the case: a folder with the file `model.skn`. Put the files that the model `use`s in the same folder. Show the source of `model.skn` to the person before the solve.
3. Optional: `skein check model.skn` in the case folder gives the type and unit errors at once, with no solve.
4. Solve: `fh run CASE`. `fh run` starts `skein report model.skn`: Skein checks the model, solves it, and writes one HTML report with the source, the tables and a viewer. The report goes to the page through the report link of the run. The tables of Skein (the solve, the `expect` checks, the memory) are in `out/skein/log.skein`.

If the run fails, the message holds the errors of Skein (a type error, a unit error, a missing `bc`, an `expect` that failed). Correct `model.skn` as each message tells, and run again. Show the person what you changed and why.

- Skein sets its own memory budget from the memory limit of `fh run` and refuses a model that does not fit. Then use a coarser `mesh` size.
- `fh run CASE --timeout SECONDS` limits the time.
- When `fh run` says that it cannot start skein, tell the person: the computer has no Skein. It needs `skein` on `PATH`, or `FH_SKEIN` set to its path. Do not write another solver's input in its place without asking.
- The report shows the results in its own viewer. A Skein run has no field for the 3D view of the page.
