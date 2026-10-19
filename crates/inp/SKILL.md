# inp skill

## Purpose

Read the part of a CalculiX `.inp` text that the preview draws, and find the files of `*INCLUDE`. CalculiX reads and checks the input itself.

## Inputs and outputs

- `parse(&str)` returns `Model`. It never fails. It takes `*NODE`, `*ELEMENT`, `*NSET`, `*ELSET`, `*SURFACE`, `*BOUNDARY`, `*CLOAD`, `*DLOAD` and `*DSLOAD`, and skips each other keyword and each data line that it cannot read.
- `flatten(path)` returns the text of the input with each `*INCLUDE` file in place, and the list of those files. An include path must be relative with no `..`.
- `units(text)` reads the `** units:` comment. `includes` and `include_path` find the include files.

## Invariants the code must keep

- The reader never refuses a keyword. A wrong input is a CalculiX error, not a reader error.
- Keyword and set names are not case sensitive.
- The reader does not declare units. The input declares them in a `** units: SI-mm` (or `SI-m`) comment (D-18).
- A test covers each supported keyword.

## Forbidden actions

- Do not describe an `.inp` writer as available. The public API exports only the reader and the model types.

## Validation hooks it must pass

Run `cargo check -p fh-inp` and `cargo test -p fh-inp`. `tests/fixtures.rs` reads the input of each validation case.

## Examples

`fh_inp::parse(&fh_inp::flatten(Path::new("job.inp"))?.text)` gives the `Model`.
