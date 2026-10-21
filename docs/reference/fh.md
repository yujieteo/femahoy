# `fh` commands

`fh` implements `mesh`, `validate`, `run`, `report`, and `serve`.

## `fh mesh`

```text
fh mesh FILE.step [-o OUT.inp] [--size H]
  [--face NAME=SEL]... [--nodes NAME=SEL]... [--volume NAME=box:X0,Y0,Z0,X1,Y1,Z1]...
  [--refine NAME[,NAME]...=H]... [--shell]
```

Source: [mesh.rs](../../crates/fh/src/mesh.rs).

| Option | Default or rule |
|---|---|
| `-o` | Input path with the extension changed to `.inp`. |
| `--size` | No value by default. Positive finite number, sent as gmsh `-clmax`. |
| `--face NAME=SEL` | Repeatable. A node set `NAME` and a surface `NAME` of the faces of the part that lie fully inside the selection. |
| `--nodes NAME=SEL` | Repeatable. A node set of the nodes in the selection. |
| `--volume NAME=box:...` | Repeatable. An element set of the elements of the bodies that lie fully inside the box. |
| `--refine NAME[,NAME]...=H` | Repeatable. Positive finite number H, below `--size`. Each `NAME` is the name of a `--face`. Needs `--size`. |
| `--shell` | Off by default. Mesh the surfaces of the STEP file as `S6` shells. |

`SEL` is a plane (`x:V`, `y:V` or `z:V`), a box (`box:X0,Y0,Z0,X1,Y1,Z1`) or, for `--nodes` only, a point (`point:X,Y,Z`).

## `fh validate`

```text
fh validate [--root DIR]
```

Sources: [validate.rs](../../crates/fh/src/validate.rs),
[measure.rs](../../crates/fh/src/validation/measure.rs), and [harness](../../crates/fh/src/validation/run.rs).

| Option | Default or rule |
|---|---|
| `--root DIR` | `.` by default. The cases are under `DIR/validation/cases`. |

## `fh run`

```text
fh run MODEL.inp|CASE [--timeout SECONDS] [--memory MIB]
```

Sources: [run.rs](../../crates/fh/src/run.rs) and [foam.rs](../../crates/fh/src/foam.rs).

`MODEL.inp` is a CalculiX input. `CASE` is an OpenFOAM case folder with `system/controlDict`. For a case, `fh run` starts `blockMesh` (when the case has `system/blockMeshDict` and no `constant/polyMesh`), then the `application` of `system/controlDict`, then `foamToVTK`. The page shows the cell fields of the last time.

| Option | Default or rule |
|---|---|
| `--timeout SECONDS` | No limit by default. A number above 0. At the limit, `fh run` stops the solver. For a case, the limit counts all the programs. |
| `--memory MIB` | 85 percent of the RAM of the machine by default. A number above 0. At the limit, `fh run` stops the solver. |

## `fh report`

```text
fh report TEXT.html -o REPORT.html --run RUN [--run RUN]...
```

Source: [report.rs](../../crates/fh/src/daemon/report.rs).

| Option | Rule |
|---|---|
| `-o REPORT.html` | Required. The command replaces the file. |
| `--run RUN` | Required, at least once. A run of `fh run` that has at least 1 result frame. The first run is the one that the viewer shows at the start. |

## `fh serve`

```text
fh serve
```

Source: [serve.rs](../../crates/fh/src/daemon/serve.rs).

`fh serve` takes no option.

## Environment variables

- `FH_GMSH`: the gmsh program of `fh mesh`.
- `FH_CCX`: the `ccx` program of `fh run` and `fh validate`.
- `FH_OPENFOAM`: the `openfoam` program of `fh run` and `fh validate`. It runs each program of OpenFOAM as `$FH_OPENFOAM PROGRAM ARGS`.
- `FH_STATE`: the state folder of `fh run`, `fh report` and `fh serve`.
- `FH_VIEWER`: the folder of the wasm viewer of `fh report` and `fh serve`.
- `FH_NO_OPEN`: a value that is not empty and not `0` stops `fh serve` from opening the browser.
