# Input keywords and element types

This reference lists the input that `fh_inp::parse` accepts.
The solver is `fh-solve-ccx`, the CalculiX adapter ([ccx.md](../ccx.md)).
“Solved” means that `fh-solve-ccx` gives the parsed data to `ccx`, subject to the limits below.
“Parsed only” means that the parser accepts the data but `fh-solve-ccx` refuses it with an error that names the keyword.
The native solver `fh-solve` solved more of this input. The owner removed it on 2026-10-19. Main commit `08a97c2` has it.

Sources: [parser](../../crates/inp/src/parse.rs), [lexer](../../crates/inp/src/lexer.rs) and
[adapter](../../crates/solver-ccx/src/main.rs).

## Keywords

These are all 22 keywords in `Parser::block`. An unlisted keyword gives `unsupported keyword`.
The parameter column lists all permitted parameter names. A dash means no parameters.

| Keyword | Parameters | Data and limits | Status |
|---|---|---|---|
| `*NODE` | `NSET` | Node ID and at least 2 coordinates. The third coordinate defaults to 0. `NSET` adds the nodes to a set. | Solved |
| `*ELEMENT` | `TYPE` (required), `ELSET` | Element ID and the exact node count for `TYPE`. Nodes must exist. `ELSET` adds the elements to a set. | Solved for 3 types. See the element table. |
| `*NSET` | `NSET` (required), `GENERATE` | Existing node IDs or earlier node sets. With `GENERATE`, use first ID, last ID, optional increment (default 1). | Solved |
| `*ELSET` | `ELSET` (required), `GENERATE` | Existing element IDs or earlier element sets. `GENERATE` follows the same rule as `*NSET`. | Solved |
| `*MATERIAL` | `NAME` (required) | No data lines. Opens a material for `*ELASTIC` and `*EXPANSION`. Material names must be unique. | Solved |
| `*ELASTIC` | `TYPE` | Exactly 1 data line with Young’s modulus and Poisson’s ratio. `TYPE` is absent or `ISOTROPIC`. | Solved |
| `*EXPANSION` | `TYPE` | Exactly 1 data line with 1 isotropic thermal expansion coefficient. `TYPE` is absent or `ISO`. The thermal strain is alpha (T - T0), with T0 the initial temperature of the node. | Solved |
| `*SOLID SECTION` | `ELSET`, `MATERIAL` (both required) | No data lines. The set and material must exist. | Solved |
| `*INITIAL CONDITIONS` | `TYPE` (required) | `TYPE=TEMPERATURE` only. Model data. Data lines: node ID or node set, temperature. | Solved |
| `*STEP` | `NAME`, `NLGEOM` | No data lines. `NLGEOM` is absent, `NO`, or `YES`. Steps cannot nest. | Solved with `NLGEOM=NO`. `YES` is parsed only. |
| `*STATIC` | — | Open step required. Optional single data line: initial increment, time period, minimum increment, maximum increment. | Solved. Each step is 1 linear increment. The adapter ignores the data line. |
| `*BOUNDARY` | — | Node ID or node set, first DOF, optional last DOF, optional value. Last DOF defaults to first. Value defaults to 0. | Solved, subject to available DOFs. Allowed outside or inside a step. |
| `*CLOAD` | — | Node ID or node set, DOF, magnitude. Open step required. | Solved, subject to available DOFs. |
| `*DLOAD` | — | Element ID or element set, `P1` to `P6`, pressure. Open step required. | Solved. Tet10 has only faces 1 to 4. |
| `*SURFACE` | `TYPE`, `NAME` (both required) | `TYPE=NODE`: data names existing nodes or node sets. `TYPE=ELEMENT`: data lines `element or element set, S<face>`. | `TYPE=ELEMENT` is solved. `*DSLOAD` names an element surface. `TYPE=NODE` is parsed only. |
| `*TEMPERATURE` | — | Node ID or node set, temperature at the end of the step. Open step required. | Solved |
| `*DSLOAD` | — | Element surface name, `P`, pressure. Open step required. The name ignores letter case. | Solved. The same pressure acts on each face of the surface. Positive pressure pushes into the body. |
| `*COUPLING` | `CONSTRAINT NAME`, `REF NODE`, `SURFACE` (all required) | No data lines. Existing reference node ID and surface required. Must immediately precede a coupling kind. | Parsed only |
| `*KINEMATIC` | — | Open coupling required. Optional single data line: first DOF, optional last DOF. Default range is 1 to 6. | Parsed only |
| `*DISTRIBUTING` | — | Same parsed DOF range as `*KINEMATIC`. | Parsed only |
| `*EQUATION` | — | First data line gives the term count. Later lines give that many node ID, DOF, coefficient triples. | Parsed only |
| `*MPC` | — | Each line gives `TIE`, `PIN`, `LINK`, or `BEAM`, then exactly 2 existing node IDs. | Parsed only |
| `*END STEP` | — | Closes an open step. The step must contain `*STATIC`. | Solved |

`*COUPLING`, `*EQUATION`, and `*MPC` refuse an open step.
A model with no `*SOLID SECTION` and 1 material gets 1 section for all elements. A model with more materials needs a `*SOLID SECTION` for each element set.
The parser accepts DOFs 1 to 6. Solid nodes have only DOFs 1 to 3.

`*ELASTIC` and `*EXPANSION` require an open material. Another keyword closes that material context.
Young’s modulus must be above 0. The parser permits Poisson’s ratio in `[-1, 0.5)`.
Repeated definitions of a node or element ID fail. Repeated definitions of a set append its members.

## Element types

| `TYPE` | Nodes | Parser | `fh-solve-ccx` |
|---|---:|---|---|
| `C3D4` | 4 | Accepted | Parsed only, for mesh display |
| `C3D8` | 8 | Accepted | Solved: the `ccx` hex8. Do not use 1 element through the depth of a section in bending. See the [mesh rule](../compatibility.md#c3d8-mesh-rule-for-bending). |
| `C3D8I` | 8 | Accepted | Solved: the `ccx` hex8 with incompatible modes |
| `C3D10` | 10 | Accepted | Solved: the `ccx` tet10 |

The parser rejects every other element type. A parsed model does not establish solver support.
`fh check` parses input. `fh validate` uses the separate `fh-solve-ccx` binary.
See the [command reference](fh.md).

## Text rules and errors

The lexer ignores blank lines and lines whose first non-space characters are `**`.
It converts keywords, parameter names, and parameter values to upper case.
It reduces spaces inside keyword names to a single space.
A trailing comma continues a line when the next non-comment, non-blank line does not start with `*`.

Unknown parameters fail. References to nodes, elements, sets, materials, and surfaces must follow their definitions where the parser checks them.
The parser reports `line N, *KEYWORD: MESSAGE`, or `line N: MESSAGE` when no keyword is known.
See [ParseError](../../crates/inp/src/error.rs).

## Keep this list current

Compare this table with `Parser::block`, each handler’s `allow` call, and `element_nodes` after a parser change.
Compare the status with `ccx_input` in the adapter after an adapter change.
Keep a parsed-only entry until the adapter supports it.
