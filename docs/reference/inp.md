# Input keywords and element types

This reference lists the input that `fh_inp::parse` accepts.
“Solved” means that the built-in solver uses the parsed data, subject to the limits below.
“Parsed only” means that the parser accepts the data but the built-in solver cannot solve it.

Sources: [parser](../../crates/inp/src/parse.rs), [lexer](../../crates/inp/src/lexer.rs),
[solver mesh](../../crates/core-solver/src/mesh.rs), [solver system](../../crates/core-solver/src/system.rs),
[step driver](../../crates/core-solver/src/step.rs), and [element kernels](../../crates/core-solver/src/element.rs).

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
| `*EXPANSION` | `TYPE` | Exactly 1 data line with 1 isotropic thermal expansion coefficient. `TYPE` is absent or `ISO`. The thermal strain is alpha (T - T0), with T0 the initial temperature of the node. | Parsed only. `fh-solve` refuses it. `fh-solve-ccx` solves it. |
| `*SOLID SECTION` | `ELSET`, `MATERIAL` (both required) | No data lines. The set and material must exist. | Solved |
| `*INITIAL CONDITIONS` | `TYPE` (required) | `TYPE=TEMPERATURE` only. Model data. Data lines: node ID or node set, temperature. | Parsed only. `fh-solve` refuses it. `fh-solve-ccx` solves it. |
| `*STEP` | `NAME`, `NLGEOM` | No data lines. `NLGEOM` is absent, `NO`, or `YES`. Steps cannot nest. | Solved with `NLGEOM=NO`. `YES` is parsed only and refused by the solver. |
| `*STATIC` | — | Open step required. Optional single data line: initial increment, time period, minimum increment, maximum increment. | Solved. The linear path uses 1 increment. |
| `*BOUNDARY` | — | Node ID or node set, first DOF, optional last DOF, optional value. Last DOF defaults to first. Value defaults to 0. | Solved, subject to available DOFs. Allowed outside or inside a step. |
| `*CLOAD` | — | Node ID or node set, DOF, magnitude. Open step required. | Solved, subject to available DOFs. |
| `*DLOAD` | — | Element ID or element set, `P1` to `P6`, pressure. Open step required. | Solved. Tet10 has only faces 1 to 4. |
| `*SURFACE` | `TYPE`, `NAME` (both required) | `TYPE=NODE`: data names existing nodes or node sets. `TYPE=ELEMENT`: data lines `element or element set, S<face>`. | Solved. A coupling can name either kind. `*DSLOAD` names an element surface. |
| `*TEMPERATURE` | — | Node ID or node set, temperature at the end of the step. Open step required. | Parsed only. `fh-solve` refuses it. `fh-solve-ccx` solves it. |
| `*DSLOAD` | — | Element surface name, `P`, pressure. Open step required. The name ignores letter case. | Solved. The same pressure acts on each face of the surface. Positive pressure pushes into the body. |
| `*COUPLING` | `CONSTRAINT NAME`, `REF NODE`, `SURFACE` (all required) | No data lines. Existing reference node ID and surface required. Must immediately precede a coupling kind. | Solved |
| `*KINEMATIC` | — | Open coupling required. Optional single data line: first DOF, optional last DOF. Default range is 1 to 6. | Solved. Constrains slave translations through reference motion. |
| `*DISTRIBUTING` | — | Same parsed DOF range as `*KINEMATIC`. | Solved. Uses equal node weights and a rotation fit when required. |
| `*EQUATION` | — | First data line gives the term count. Later lines give that many node ID, DOF, coefficient triples. | Solved as a homogeneous linear constraint. |
| `*MPC` | — | Each line gives `TIE`, `PIN`, `LINK`, or `BEAM`, then exactly 2 existing node IDs. | Solved. `TIE` and `PIN` equate translations. `LINK` constrains axial motion. `BEAM` includes rotations. |
| `*END STEP` | — | Closes an open step. The step must contain `*STATIC`. | Solved |

`*COUPLING`, `*EQUATION`, and `*MPC` refuse an open step.
The solver requires a `*SOLID SECTION` for each element and at least 1 step.
The parser accepts DOFs 1 to 6. Ordinary solid nodes have only DOFs 1 to 3.
Coupling reference nodes and `BEAM` ends can have rotation DOFs.
The solver refuses a DOF that the node does not have.

`*ELASTIC` and `*EXPANSION` require an open material. Another keyword closes that material context.
Young’s modulus must be above 0. The parser permits Poisson’s ratio in `[-1, 0.5)`.
Repeated definitions of a node or element ID fail. Repeated definitions of a set append its members.

## Element types

| `TYPE` | Nodes | Parser | Built-in solver |
|---|---:|---|---|
| `C3D4` | 4 | Accepted | Parsed only for mesh display. The solver reports that the type is not available yet. |
| `C3D8` | 8 | Accepted | Solved: hex8 with B-bar. Do not use 1 element through the depth of a section in bending. See the [mesh rule](../compatibility.md#c3d8-mesh-rule-for-bending). |
| `C3D8I` | 8 | Accepted | Solved: hex8 with incompatible modes |
| `C3D10` | 10 | Accepted | Solved: tet10 with 4 integration points |

The parser rejects every other element type. A parsed model does not establish solver support.
`fh check` parses input. `fh validate` uses the separate `fh-solve` binary.
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
Compare solver status with `Kind::parse`, `Mesh`, and `solve_run` after a solver change.
Keep a parsed-only entry until the executable solver path supports it.
