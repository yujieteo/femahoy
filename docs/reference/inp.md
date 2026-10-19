# Input keywords and element types

This reference lists the input that `fh_inp::parse` accepts.
The solver is CalculiX (`ccx`), through the adapter crate `solver-ccx` ([ccx.md](../ccx.md)).
“Solved” means that the adapter gives the parsed data to `ccx`, subject to the limits below.
The native solver `fh-solve` solved more of this input. The owner removed it on 2026-10-19. Main commit `08a97c2` has it.

Sources: [parser](../../crates/inp/src/parse.rs), [lexer](../../crates/inp/src/lexer.rs) and
[adapter](../../crates/solver-ccx/src/main.rs).

## Keywords

These are all 18 keywords in `Parser::block`. An unlisted keyword gives `unsupported keyword`.
The parameter column lists all permitted parameter names. A dash means no parameters.

| Keyword | Parameters | Data and limits | Status |
|---|---|---|---|
| `*NODE` | `NSET` | Node ID and at least 2 coordinates. The third coordinate defaults to 0. `NSET` adds the nodes to a set. | Solved |
| `*ELEMENT` | `TYPE` (required), `ELSET` | Element ID and the exact node count for `TYPE`. Nodes must exist. `ELSET` adds the elements to a set. | Solved. `TYPE` is `C3D10` only. |
| `*NSET` | `NSET` (required), `GENERATE` | Existing node IDs or earlier node sets. With `GENERATE`, use first ID, last ID, optional increment (default 1). | Solved |
| `*ELSET` | `ELSET` (required), `GENERATE` | Existing element IDs or earlier element sets. `GENERATE` follows the same rule as `*NSET`. | Solved |
| `*MATERIAL` | `NAME` (required) | No data lines. Opens a material for `*ELASTIC` and `*EXPANSION`. Material names must be unique. | Solved |
| `*ELASTIC` | `TYPE` | Exactly 1 data line with Young’s modulus and Poisson’s ratio. `TYPE` is absent or `ISOTROPIC`. | Solved |
| `*EXPANSION` | `TYPE` | Exactly 1 data line with 1 isotropic thermal expansion coefficient. `TYPE` is absent or `ISO`. The thermal strain is alpha (T - T0), with T0 the initial temperature of the node. | Solved |
| `*SOLID SECTION` | `ELSET`, `MATERIAL` (both required) | No data lines. The set and material must exist. | Solved |
| `*INITIAL CONDITIONS` | `TYPE` (required) | `TYPE=TEMPERATURE` only. Model data. Data lines: node ID or node set, temperature. | Solved |
| `*STEP` | `NAME` | No data lines. Steps cannot nest. | Solved |
| `*STATIC` | — | Open step required. 1 `*STATIC` in each step. The parser does not read the data lines. | Solved. Each step is 1 linear increment. |
| `*BOUNDARY` | — | Node ID or node set, first DOF, optional last DOF, optional value. Last DOF defaults to first. Value defaults to 0. | Solved, subject to available DOFs. Allowed outside or inside a step. |
| `*CLOAD` | — | Node ID or node set, DOF, magnitude. Open step required. | Solved, subject to available DOFs. |
| `*DLOAD` | — | Element ID or element set, `P1` to `P4`, pressure. Open step required. | Solved |
| `*SURFACE` | `TYPE`, `NAME` (both required) | `TYPE=ELEMENT` only. Data lines: `element or element set, S<face>`, with face 1 to 4. | Solved. `*DSLOAD` names an element surface. |
| `*TEMPERATURE` | — | Node ID or node set, temperature at the end of the step. Open step required. | Solved |
| `*DSLOAD` | — | Element surface name, `P`, pressure. Open step required. The name ignores letter case. | Solved. The same pressure acts on each face of the surface. Positive pressure pushes into the body. |
| `*END STEP` | — | Closes an open step. The step must contain `*STATIC`. | Solved |

A model with no `*SOLID SECTION` and 1 material gets 1 section for all elements. A model with more materials needs a `*SOLID SECTION` for each element set.
The parser accepts DOFs 1 to 6. Solid nodes have only DOFs 1 to 3.

`*ELASTIC` and `*EXPANSION` require an open material. Another keyword closes that material context.
Young’s modulus must be above 0. The parser permits Poisson’s ratio in `[-1, 0.5)`.
Repeated definitions of a node or element ID fail. Repeated definitions of a set append its members.

## Element types

The only element type is `C3D10` (10 nodes, D-19). `ccx` solves it as its tet10.
The parser rejects every other element type with `unsupported element type TYPE; only C3D10 is supported`.
`fh validate` starts the separate `ccx` program. See the [command reference](fh.md).

## Text rules and errors

The lexer ignores blank lines and lines whose first non-space characters are `**`.
It converts keywords, parameter names, and parameter values to upper case.
It reduces spaces inside keyword names to a single space.
A trailing comma continues a line when the next non-comment, non-blank line does not start with `*`.

Unknown parameters fail. References to nodes, elements, sets, materials, and surfaces must follow their definitions where the parser checks them.
The parser reports `line N, *KEYWORD: MESSAGE`, or `line N: MESSAGE` when no keyword is known.
See [ParseError](../../crates/inp/src/error.rs).

## Keep this list current

Compare this table with `Parser::block` and each handler’s `allow` call after a parser change.
Compare the status with `ccx_input` in the adapter after an adapter change.
