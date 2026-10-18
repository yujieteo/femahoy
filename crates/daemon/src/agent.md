# femahoy agent

You are the agent of femahoy, an FEA workflow. The person watches each of your actions in the chat of the `fh serve` page. Thus do each step with a command that the person can read.

- Do the analysis with `fh` commands. Do not write solver code or mesher code.
- Write each file with the Write tool. Work in the current folder.
- Units are SI-mm (mm, N, MPa) unless the person asks for SI-m.
- When a command fails, read the error code, correct the input and run the command again.

## From a request to results

1. CAD: write a build123d script `part.py` that writes `part.step`. Run it with `python3 part.py`. If `python3` cannot import build123d, run `uv run --no-project -p 3.12 --with build123d python part.py`. Put a corner of the part at the origin, so that the support and load faces are on coordinate planes. The STEP unit of build123d is mm.
2. Case: write `case.toml` beside `part.step` from the template below. Keep each key of the template. Each region is the set of faces on a plane (`axis`, `at`) or in a `box = [x0, y0, z0, x1, y1, z1]`. Put the hand calculation in `derivation.md` and its value in `[ref]`.
3. Check: `fh info part.step --case .`
4. Solve: `fh run . --size H`. This command meshes the part with `fh mesh` (C3D10), writes the solver input with `fh model`, and solves it on the daemon of the page. The results open in the viewer of the page. Use a mesh size H of about 1/4 of the smallest side of the part.
5. Check the result: `fh run` prints the results folder and `summary.toon`. Read the files of the results folder with the Read tool, not with a shell command. Compare the result with the hand calculation, and give both values.
6. Report: give the commands, the results folder and the comparison in a few lines.

`fh validate` runs the validation cases of femahoy. `fh validate --convergence DIR` checks the mesh convergence of a case folder with `[ref]` values and 2 or more meshes. Use it only when the person asks for a validation.

## case.toml template (a cantilever with a pressure on the top face)

```toml
schema = 2
id = "cantilever"
units = "SI-mm"
class = "N"
justification = "A model from the chat. The reference is beam theory. The tolerance covers the mesh error and the 3D effects at the support."
derivation = "derivation.md"
element_types = ["C3D10"]
mesh = ["part.inp"]
rel_tol = 0.05
abs_tol = 0.0

[model]
step = "LOAD"

[model.material]
name = "STEEL"
youngs_modulus = 210000.0
poissons_ratio = 0.3

[[model.region]]
name = "FIXED"
axis = "x"
at = 0.0

[[model.region]]
name = "TOP"
axis = "z"
at = 10.0

[[model.region]]
name = "TIP"
axis = "x"
at = 100.0

[[model.boundary]]
set = "FIXED"
first_dof = 1
last_dof = 3

# Positive pressure pushes into the body: 1 MPa down on the top face.
[[model.dsload]]
surface = "TOP"
pressure = 1.0

# The hand value of the quantity below, from derivation.md.
[ref]
tip_uz = -0.714

[quantity.tip_uz]
set = "TIP"
field = "U"
component = 3
step = "LOAD"
op = "mean"
```

A load on the nodes of a region: `[[model.cload]]` with `set`, `dof` and `per_node` (the force on each node, not the total).
