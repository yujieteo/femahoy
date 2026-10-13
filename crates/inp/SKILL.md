# inp skill

## Purpose

Parse the supported Abaqus-style `.inp` subset into `Model`.

## Inputs and outputs

- `parse(&str)` returns `Model` or `ParseError` with a line and keyword.
- The subset includes nodes, elements, sets, elastic materials, solid sections, static steps, boundaries, concentrated loads and distributed loads.
- Node surfaces, couplings, equations and MPCs are supported.
- The parser accepts `C3D8`, `C3D8I`, `C3D10` and `C3D10M`. Parser acceptance does not imply solver support.

## Invariants the code must keep

- Unsupported keywords and duplicate node IDs fail.
- An open step needs `*END STEP`.
- The parser does not declare units. Validation cases declare units in `case.toml`.

## Forbidden actions

Do not describe an `.inp` writer or template mapper as available. The public API exports only the parser and model types.

## Validation hooks it must pass

Run `cargo check -p fh-inp` and `cargo test -p fh-inp`. `fh check --root .` checks repository fixtures.

## Examples

`fh check FILE.inp` checks a file and prints text counts or errors.
