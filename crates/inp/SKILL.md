# inp skill

## Purpose

Parse the supported Abaqus-style `.inp` subset into `Model`.

## Inputs and outputs

- `parse(&str)` returns `Model` or `ParseError` with a line and keyword.
- The subset includes nodes, elements, sets, elastic materials, solid sections, static steps, boundaries, concentrated loads and distributed loads.
- The only element is `C3D10` (D-19). Another element type fails with a clear error.
- `*SURFACE` accepts only `TYPE=ELEMENT`. `*STEP` accepts no `NLGEOM`. `*STATIC` data lines are accepted and not used.

## Invariants the code must keep

- Unsupported keywords and duplicate node IDs fail.
- An open step needs `*END STEP`.
- The parser does not declare units. Validation cases declare units in `case.toml`.
- Each unsupported keyword fails with a clear error that names the keyword and the line number (DESIGN.md section 7.3).
- Each difference from Abaqus is listed in `docs/compatibility.md`, and a test covers it (DESIGN.md section 7.4).

## Forbidden actions

- Do not describe an `.inp` writer as available. The public API exports only the parser and model types.

## Validation hooks it must pass

Run `cargo check -p fh-inp` and `cargo test -p fh-inp`. `tests/fixtures.rs` parses the mesh of each validation case.

## Examples

`fh_inp::parse_file("model.inp", &text)` gives the `Model` or the list of errors.
