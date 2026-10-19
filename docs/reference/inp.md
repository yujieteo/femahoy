# Input keywords and element types

CalculiX (`ccx`) reads and checks the input. It takes every keyword, element type and solver that it has.
`fh run` gives `ccx` the input file unchanged, with each file that it includes (`*INCLUDE`).

The crate `fh-inp` has a small reader for the preview of the supports and the loads. It never refuses a keyword.
It takes the keywords below, and skips each other keyword and each data line that it cannot read.

Sources: [reader](../../crates/inp/src/parse.rs), [lexer](../../crates/inp/src/lexer.rs) and
[files](../../crates/inp/src/files.rs).

## Keywords for the preview

| Keyword | Parameters | Data |
|---|---|---|
| `*NODE` | `NSET` | Node ID and the coordinates. `NSET` adds the nodes to a set. |
| `*ELEMENT` | `TYPE`, `ELSET` | Element ID and its node IDs. Any `TYPE`. `ELSET` adds the elements to a set. |
| `*NSET` | `NSET`, `GENERATE` | Node IDs or earlier node sets. With `GENERATE`: first ID, last ID, optional increment. |
| `*ELSET` | `ELSET`, `GENERATE` | Element IDs or earlier element sets. `GENERATE` follows the same rule. |
| `*SURFACE` | `TYPE=ELEMENT`, `NAME` | `element or element set, S<face>` (or `SPOS`, `SNEG`). |
| `*STEP`, `*END STEP` | `NAME` | Open and close a step. The loads of a step are in the step. |
| `*BOUNDARY` | — | Node ID or node set, first DOF, optional last DOF, optional value. Outside a step or inside a step. |
| `*CLOAD` | — | Node ID or node set, DOF, magnitude. In a step. |
| `*DLOAD` | — | Element ID or element set, `P1` to `P6`, pressure. In a step. |
| `*DSLOAD` | — | Element surface name, `P`, pressure. In a step. Positive pressure pushes into the body. |
| `*INCLUDE` | `INPUT` | A file in the folder of the input, or in a folder below it. The path is relative, with no `..`. |

Names of keywords, parameters, sets and surfaces ignore letter case.
The lexer ignores blank lines and lines that start with `**`. A trailing comma continues a line.

## Element types

The page and the preview draw the element types that `ccx` writes to the result file: `C3D4`, `C3D6`, `C3D8`, `C3D10`, `C3D15` and `C3D20`.
`ccx` writes a shell or a beam element (for example `S8R`, `S4` or `B32`) as one of these solid elements, so the page shows it too. The adapter also converts the 2D and line cells of the result file, but the page does not draw them.
`fh mesh` writes `C3D10`.

## Units

`** units: SI-mm` (or `SI-m`) is a comment line in the input. `fh mesh` writes it. The run copies it into `results.pvd`.
