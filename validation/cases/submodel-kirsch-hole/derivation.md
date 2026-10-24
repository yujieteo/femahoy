# submodel-kirsch-hole derivation

This case checks a submodel: a fine model of the region at a hole that gets its displacement on the cut boundary from a coarse global model (units SI-mm).
The coarse global model alone gives the peak stress at the hole 4.5 % low. The submodel gives it 0.14 % high, and 0.029 % high at the integration point of the check.

## Source

G. Kirsch, "Die Theorie der Elastizität und die Bedürfnisse der Festigkeitslehre", Zeitschrift des Vereines deutscher Ingenieure 42 (1898) 797-807: a circular hole in an infinite plate in tension.
The solution is also in S. P. Timoshenko and J. N. Goodier, *Theory of Elasticity*, 3rd ed., McGraw-Hill, 1970, article 35.
For the tension σ along x and a hole of radius a, in polar coordinates r, θ:

- σ_rr = σ/2 (1 - a²/r²) + σ/2 (1 - 4a²/r² + 3a⁴/r⁴) cos 2θ
- σ_θθ = σ/2 (1 + a²/r²) - σ/2 (1 + 3a⁴/r⁴) cos 2θ
- σ_rθ = -σ/2 (1 + 2a²/r² - 3a⁴/r⁴) sin 2θ

At the top of the hole, (0, a), σ_xx = σ_θθ = 3σ.
The displacement of this stress field, with μ the shear modulus and κ = 3 - 4ν in plane strain, is:

- u_x = σ a / (8μ) [(r/a)(κ + 1) cos θ + 2 (a/r)((1 + κ) cos θ + cos 3θ) - 2 (a/r)³ cos 3θ]
- u_y = σ a / (8μ) [(r/a)(κ - 3) sin θ + 2 (a/r)((1 - κ) sin θ + sin 3θ) - 2 (a/r)³ sin 3θ]

The strains of this displacement give the 3 stresses above. A check with central differences at 6 points gave a difference of less than 10⁻⁷ MPa.

| Item | Value |
|---|---|
| σ, the tension along x | 100 MPa |
| a, the radius of the hole | 10 mm |
| Steel | E = 210000 MPa, ν = 0.3, plane strain |
| σ_xx at (0, a) | 300 MPa |

In plane strain and in plane stress, the in-plane stresses are the same. Only the displacement changes (κ = (3 - ν)/(1 + ν) in plane stress).

## Model

The 2 models are 1/4 of the plate, with symmetry on x = 0 and y = 0. Both use `CPE8R` elements (plane strain), 1 mm thick.

| Item | Global model (`global.inp`) | Submodel (`job.inp`) |
|---|---|---|
| Region | 0 <= x, y <= 50 mm, minus the hole | the ring 10 <= r <= 20 mm |
| Mesh | 16 elements, 65 nodes. 2 blocks (0 to 45° and 45 to 90°), each with 2 elements along the hole and 4 to the outer edge, each 1.5 times longer than the one before | 288 elements, 937 nodes. 24 elements along the hole and 12 to the cut, each 1.2 times longer than the one before |
| Element at the hole | 3.9 mm along the hole, 4.9 mm radial | 0.65 mm along the hole, 0.25 mm radial |
| `LEFT`, x = 0 | ux = 0 | ux = 0 |
| `BOTTOM`, y = 0 | uy = 0 | uy = 0 |
| Outer boundary | `EDGE` (x = 50 mm and y = 50 mm): u_x and u_y of Kirsch | `CUT` (r = 20 mm): the displacement of the global model |

The outer edges of the global model get the exact displacement of the infinite plate. Thus the global model has no error from the size of the plate, and the exact solution is the Kirsch solution at each point of both models.

## The submodel in CalculiX

`job.inp` has these lines:

```
*SUBMODEL, TYPE=NODE, INPUT=global.frd
CUT
...
*BOUNDARY, SUBMODEL, STEP=1
CUT, 1, 2
```

`ccx` reads the result file `global.frd` of the global model, finds the global element that holds each node of `CUT`, and interpolates the displacement of step 1 at the node.
The submodel has no other load.

`ccx` reads `global.frd` in the folder where it runs. Thus `fh` copies `global.inp` (the file with the name of `global.frd` and the extension `.inp`) and its `*INCLUDE` files with `job.inp`, and solves `global.inp` first in the same folder.
`global.inp` writes `*NODE FILE, OUTPUT=3D`. `ccx` makes a 3D element of each 2D element, and the submodel finds its nodes in the 3D elements of `global.frd`. With the 2D elements of the default output, the submodel stopped with `*ERROR in dsort: the number of values to be sorted is not positive`.

## The check

CalculiX writes the stress only at the integration points (`*EL PRINT`). Thus the check compares with the exact stress at an integration point.
`TOP` is the element of the submodel at the hole next to (0, a). The check is the largest σ_xx of its integration points.
`mesh.py` finds the 2 x 2 integration points of `TOP` (±1/√3 in the element) and prints the Kirsch σ_xx at the point nearest (0, a): x = 0.139 mm, y = 10.052 mm, σ_xx = 296.179 MPa.

The result is σ_xx = 296.265 MPa, 0.029 % above the reference.
The nodal value of `S11` at (0, a) in `job.frd` (the mean of the elements at the node) is 300.41 MPa, 0.14 % above 3σ.
The global model alone gives 286.51 MPa at (0, a) in `global.frd`, 4.5 % below 3σ.

`curves.py` writes σ_xx on the line x = 0 of the 3 solutions (the nodal `S11` of `global.frd` and `job.frd` of the run) to the curve `line-x0.csv` of the report.
At y = 12.46 mm, the second node of the global model, the global model gives 8 % above Kirsch.
From the hole to y = 18 mm, the submodel stays within 0.4 % of Kirsch at its nodes on the line.
Near the cut, the submodel has the error of the global model: -1.0 % at y = 19.1 mm and -3.3 % at the cut, y = 20 mm. Thus read the results of a submodel away from its cut.

## Studies

Each study changes 1 option of `mesh.py` and solves the 2 models with `ccx`. "Exact cut" is the submodel with the Kirsch displacement on `CUT` in place of the global model: `python3 mesh.py --exact-cut cut-exact.inp` writes it, and an `*INCLUDE, INPUT=cut-exact.inp` replaces the 2 submodel keywords.
The exact cut gives the error of the submodel mesh. The difference between the 2 columns is the error of the displacement of the global model on the cut.

### Mesh of the submodel

Each finer mesh has 2 times more elements in each direction. The element `TOP` and its reference change with the mesh.

| `--sub` | Reference (MPa) | Global model at the cut | Exact cut | Nodal S11 at (0, a) |
|---|---:|---:|---:|---:|
| 12 6 | 285.343 | +0.046 % | +0.018 % | -0.07 % |
| 24 12 (this case) | 296.179 | +0.029 % | +0.000 % | +0.14 % |
| 48 24 | 299.590 | +0.029 % | +0.000 % | +0.07 % |
| 96 48 | 299.987 | +0.029 % | | +0.04 % |

From 24 x 12 elements, the submodel mesh adds less than 0.001 %. The error of 0.029 % comes from the global model.
From 24 x 12 elements, the nodal value gets nearer to 3σ as the mesh gets finer, because the extrapolation from the integration points to the node gets shorter.

### Mesh of the global model

| `--global` | Elements | Global model alone, S11 at (0, a) | Submodel, check |
|---|---:|---:|---:|
| 1 2 | 4 | -24.1 % | +0.38 % |
| 1 3 | 6 | -10.8 % | -1.73 % |
| 2 4 (this case) | 16 | -4.5 % | +0.029 % |
| 4 8 | 64 | +0.9 % | +0.016 % |
| 8 16 | 256 | +0.4 % | +0.000 % |

The submodel is only as good as the displacement of the global model on the cut. With 1 element along each 45° of the hole, the submodel has an error of 0.4 % to 1.7 %.

### Radius of the cut

| `--cut` (mm) | R/a | Reference (MPa) | Global model at the cut | Exact cut |
|---|---:|---:|---:|---:|
| 12.5 | 1.25 | 298.936 | +0.36 % | |
| 15 | 1.5 | 298.011 | -1.02 % | -0.000 % |
| 20 (this case) | 2 | 296.179 | +0.029 % | +0.000 % |
| 25 | 2.5 | 294.370 | -0.06 % | |
| 30 | 3 | 292.583 | -0.05 % | +0.003 % |
| 40 | 4 | 289.076 | +0.005 % | |

Near the hole, the coarse global model has a large error of the displacement, and the cut gives it to the submodel. From R = 2a, the error is 0.06 % or less.
The error does not fall steadily with R. It follows the error of the global displacement on the cut: the largest difference to Kirsch on `CUT` is 0.47 % of the largest displacement at R = 12.5 mm, 0.86 % at 15 mm and 0.27 % at 20 mm.

Plane stress (`CPS8R`, 1 mm thick) with the exact cut gave +0.16 %, not 0.000 %. `ccx` makes a layer of this thickness from each 2D element, and the elements at the hole are smaller than the thickness. Thus the case uses plane strain.

## Tolerance

The tolerance is 0.1 % (`rel_tol = 0.001`). It is the sum of the error of the global displacement on the cut (0.03 %) and the error of the submodel mesh (0.001 %), with a margin of 3 times.
An error of the method is larger: a cut at R = 1.5a gives -1.0 %, a global model with 6 elements gives -1.7 %, and the coarse global model alone gives -4.5 % at the node.

## Files

`mesh.inp` and `global-mesh.inp` come from `python3 mesh.py`. The studies use the options of `mesh.py`, for example `python3 mesh.py --global 4 8` or `python3 mesh.py --cut 30`.
