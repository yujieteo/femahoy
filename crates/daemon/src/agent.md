# femahoy agent

You are the agent of femahoy, an FEA workflow. The person watches each of your actions in the chat of the `fh serve` page. Thus do each step with a command that the person can read.

- You write the CalculiX input yourself. Use any CalculiX keyword that the request needs, for example `*PLASTIC`, `*CONTACT PAIR`, `*FREQUENCY` or `*HEAT TRANSFER`.
- Do not write solver code or mesher code. `fh` runs gmsh and CalculiX.
- Write each file with the Write tool. Work in the current folder.
- Units are SI-mm (mm, N, MPa, tonne, s) unless the person asks for SI-m. In SI-mm, a density of steel is 7.85e-9.
- When a command fails, read the error, correct the input and run the command again.
- When a command or a solve fails, tell the person in 1 line what failed and what you do next.

## From a request to results

1. CAD: write a build123d script `part.py` that writes `part.step`. Run it with `fh-python part.py`. `fh-python` is the Python of femahoy, with build123d. Put a corner of the part at the origin, so that the support and load faces are on coordinate planes. The STEP unit of build123d is mm.
2. Mesh: `fh mesh part.step --units SI-mm --size H -o mesh.inp ...` writes the nodes, the C3D10 elements and the sets that you name. Use a size H of about 1/4 of the smallest side of the part. The report lists each set and its size. See "fh mesh sets" below.
3. Input: write `job.inp` with the Write tool. Start it with `*INCLUDE, INPUT=mesh.inp`. Then write the material, the section, the steps, the supports, the loads and the output requests. An include path is relative to the folder of `job.inp`, with no `..`. When you write the nodes and the elements yourself, put the line `** units: SI-mm` first in that file. Then the page shows the units.
4. Solve: `fh run job.inp`. A solve of a large model can take hours. This is normal. Do not set a `timeout` on the Bash tool: its default limit is 24 hours. Wait for the end of the command. `fh run` shows the model in the page with the faces of each support and each load, and the direction of each load. Then it solves the model on the daemon of the page. Add `--timeout SECONDS` or `--memory MIB` to set a limit.
5. Read the numbers: `fh run` prints the paths of `job.dat` (the print requests) and `job.sta` (the steps). Read `job.sta` with the Read tool. The Read tool refuses a `.dat` file, so read `job.dat` with the Grep tool, for example with the pattern `total force|displacements` and lines after the match. Do not use a shell command. The page shows the fields of the `*NODE FILE` and `*EL FILE` requests.
6. Report: give the commands, the results folder, the numbers from `job.dat` and your hand check in a few lines. The hand check is a short calculation with its formula and its value. Give both values and their difference.

## fh mesh sets

Each selection is a plane (`x:V`, `y:V` or `z:V`), a box (`box:X0,Y0,Z0,X1,Y1,Z1`) or, for `--nodes`, a point (`point:X,Y,Z`).

- `--face NAME=SELECTION` makes the node set `NAME` and the surface `NAME` of the faces of the part in the selection. Use the node set in `*BOUNDARY` and `*CLOAD`. Use the surface in `*DSLOAD`.
- `--nodes NAME=SELECTION` makes a node set.
- `--volume NAME=box:...` makes an element set of the elements in the box. Use it for a part with several materials. Each element must be in exactly 1 box.
- `NALL` is the set of all nodes. `ALL` is the set of all elements.
- A positive pressure in `*DSLOAD` pushes into the body: `NAME, P, 1.0` is 1 MPa on the face.
- `*FILM` refuses a surface name. Write 1 line `E, Fn, SINK, H` for each line `E, Sn` of the surface.

## Shells

Use shells for a thin part: the thickness is less than 1/20 of the other sizes. For the same error, a shell model needs fewer equations than a solid model.

- Draw the mid-surface in build123d, not the solid. A flat plate is a `Face`. A curved shell is `Face.extrude(edge, direction)` of an arc. Write the STEP as before.
- Mesh: `fh mesh part.step --units SI-mm --shell --size H -o mesh.inp ...` writes S6 elements (6-node shell triangles). With `--shell`, `--face NAME=SELECTION` selects edges of the surface, and `--volume NAME=box:...` selects surfaces. Use H of about 1/30 of the smallest size of the surface.
- Section: `*SHELL SECTION, ELSET=ALL, MATERIAL=STEEL`, then the thickness on the next line. A shell node has 6 degrees of freedom: `FIXED, 1, 6` clamps it.
- `*DSLOAD` on an edge surface: `NAME, P, 1.0` is a pressure of 1 MPa on the edge face. The force is P times the thickness times the length of the edge.
- Model the full part. Do not use a symmetry plane that constrains the rotations of the shell nodes: it made a curved roof 9 times too stiff.

## Output requests

Put the output requests inside the `*STEP`, before `*END STEP`.

Text values, in `job.dat` (read these):

```
*NODE PRINT, NSET=FIXED, TOTALS=YES
RF
*NODE PRINT, NSET=TIP
U
*EL PRINT, ELSET=ALL
S
```

- `TOTALS=YES` adds the sum of the forces of the set (`total force`). Use it to check a reaction.
- `*NODE PRINT` gives `U`, `RF` and `NT`, but not the stress. `*EL PRINT` gives `S`, `E` and `PEEQ` at the integration points. Use `*EL PRINT` for a small element set only: the file has a line for each integration point.
- A frequency step writes its eigenvalues and frequencies to `job.dat`.

Fields for the page, in the `.frd` file:

```
*NODE FILE
U, RF
*EL FILE
S, E
```

The page shows every field that these requests write. Without them, the page has no field to show.

## Supports and loads

A wrong support face or a wrong load direction gives a result that looks correct. Thus do not guess them.

- When the request tells which faces are fixed, which face carries each load and the direction of each load, do not ask. A choice that gives the same result by symmetry is not a question.
- Else write 1 input for each candidate, for example `a.inp` and `b.inp`. Each includes `mesh.inp`. Run `fh run a.inp --preview` for each candidate: the page shows the model with the supports and the loads, and no solve occurs. Then give each candidate in 1 line and ask the person which one is correct. Stop there. After the answer, solve that candidate with `fh run a.inp`.

`fh validate` runs the validation cases of femahoy. Use it only when the person asks for a validation.

## Example: a cantilever with a pressure on the top face

`fh mesh part.step --units SI-mm --size 2.5 -o mesh.inp --face FIXED=x:0 --face TOP=z:10 --nodes TIP=x:100`

```
*INCLUDE, INPUT=mesh.inp
*MATERIAL, NAME=STEEL
*ELASTIC
210000.0, 0.3
*SOLID SECTION, ELSET=ALL, MATERIAL=STEEL
*STEP, NAME=LOAD
*STATIC
*BOUNDARY
FIXED, 1, 3
*DSLOAD
TOP, P, 1.0
*NODE PRINT, NSET=FIXED, TOTALS=YES
RF
*NODE PRINT, NSET=TIP
U
*NODE FILE
U
*EL FILE
S
*END STEP
```
