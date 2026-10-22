# cht-heated-channel derivation

This case checks conjugate heat transfer in the CFD path: a fluid region and a solid region that exchange heat through a common interface.
Laminar air flows between 2 parallel plastic walls. A heater on the outer face of each wall gives a uniform heat flux. The heat goes through the wall and into the air.
The reference is exact. Thus the tolerance comes from the mesh study of the model, not from the difference between the model and the reference.

## Sources

- R. K. Shah and A. L. London, *Laminar Flow Forced Convection in Ducts*, Advances in Heat Transfer, Supplement 1, Academic Press, 1978.
  For fully developed laminar flow between parallel plates with the same uniform heat flux on the 2 plates, Nu = h Dh / k = 140/17 = 8.235, with Dh = 4b and b the half of the distance between the plates.
- The derivation of this value is short. The velocity is u = (3/2) U (1 - y²/b²). In the fully developed state, dT/dx = G at each y, and ρ cp u G = k d²T/dy². The energy balance of the cross section gives ρ cp U b G = q. Integration across the half channel gives T_w - T_b = 17 q b / (35 k) and T_w - T_c, with T_c on the centre line, = 5 q b / (8 k).
- In the wall, the temperature also rises by G along x. Thus the heat flux across the wall is q at each x, and the temperature is linear across the wall: T_heater - T_w = q t / k_s.
- The energy balance from the inlet to the station x0 gives the bulk temperature there. The heat that goes into the channel is q (x0 - x_s), with x_s the start of the heater. Heat also goes upstream through the section x0, by conduction along x in the air and in the wall. In the fully developed region, this conduction is (k_f b + k_s t) G.

The published conjugate benchmarks that the study found give their results in figures, or are not open to read. A value from a figure has an error that is not known. The exact solution has no error. Its conjugate effects are large against the tolerance:

- The conduction along the wall adds 0.52 K to the bulk temperature at x0. A model with no conduction along the wall gives an interface_T that is 0.52 K too low.
- The wall adds 0.48 K between the heater and the interface. A model with no wall resistance gives a heater_T that is 0.48 K too low.

## Model

The OpenFOAM case `channel` has the regions `fluid` and `solid` (`constant/regionProperties`). Each region has its own `system/<region>/blockMeshDict`, `fvSchemes` and `fvSolution`, its own `constant/<region>` and its own `0/<region>`.

| Item | Value |
|---|---|
| Fluid region | the half channel 0 <= y <= b = 1 mm, 0 <= x <= 90 mm, with 1 cell of depth 0.1 mm in z and an `empty` patch on the front and the back |
| Solid region | the wall 1 <= y <= 2 mm (t = 1 mm), 0 <= x <= 90 mm |
| Air | ρ = 1.2 kg/m³ (`rhoConst`), cp = 1000 J/(kg K), μ = 1.8e-5 Pa s, Pr = 0.72. Thus k_f = μ cp / Pr = 0.025 W/(m K) |
| Wall | k_s = 0.25 W/(m K). ρ = 1200 kg/m³ and cp = 1000 J/(kg K) do not change a steady state |
| Inlet | x = 0: uniform U = 0.2 m/s and T_in = 300 K. Re = ρ U Dh / μ = 53.3 and Pe = Re Pr = 38.4 |
| Outlet | x = 90 mm: p = 1e5 Pa, zero gradient of U and T |
| Centre line | y = 0: `symmetryPlane` |
| Interface | y = 1 mm: `mappedWall` patches in the 2 regions. No slip. `compressible::turbulentTemperatureRadCoupledMixed` on T in the 2 regions |
| Heater | the outer face y = 2 mm on 20 <= x <= 80 mm: `externalWallHeatFluxTemperature`, q = 120 W/m² |
| Other faces of the wall | adiabatic |
| Solver | `chtMultiRegionSimpleFoam`, laminar and steady, g = 0 |
| Schemes | `bounded Gauss linear` for `div(phi,U)`, `div(phi,h)` and `div(phi,K)`, `Gauss linear` for the gradients, `Gauss linear corrected` for the Laplacians |
| Relaxation | 0.7 for `U` and `p_rgh` and 0.9 for `h` in the fluid, 1 for `h` in the solid |
| Mesh | uniform, n = 10 cells across each region, and the cells are 2 times as long as high: 450 x 10 cells in each region, 9000 in total |
| Iterations | 2000 |

The station of the checks is x0 = 40 mm. The function objects of `system/controlDict` write the 3 values:

- `interfaceT`: the area average of T on the fluid patch `interface_window` (38 <= x <= 42 mm). T is linear in x there, thus the average is the value at x0.
- `heaterT`: the area average of T on the solid patch `heater_window` (38 <= x <= 42 mm).
- `outletT`: the average of T on the patch `outlet`, with the mass flux `phi` as the weight. This is the bulk temperature.

| Quantity | Formula | Value |
|---|---|---:|
| G | q / (ρ cp U b) | 500 K/m |
| T_b at x0 | T_in + [q (x0 - x_s) + (k_f b + k_s t) G] / (ρ cp U b) | 310.57292 K |
| T_w - T_b | 17 q b / (35 k_f) | 2.33143 K |
| `interface_T` | T_b + 17 q b / (35 k_f) | 312.90435 K |
| `heater_T` | `interface_T` + q t / k_s | 313.38435 K |
| `outlet_T` | T_in + q (x_e - x_s) / (ρ cp U b), with x_e = 80 mm the end of the heater | 330 K |

The station must be far from the 2 ends of the heater. The wall moves the effect of an end along x by conduction, also upstream.
A first model had the heater end at 60 mm, 18 mm after the window. There, at n = 10, `interface_T` was 0.0019 K lower and the wall drop was 0.00013 K lower.
With the heater end at 80 mm, a move of the end to 100 mm changes `interface_T` by 1.4e-6 K. When the window is 10 mm further from the heater start, `interface_T` is 7e-5 K nearer to the exact value.

## Mesh study

These commands, in the repository root, solve the case on a mesh with n = 20 in a scratch folder:

```
cp -R validation/cases/cht-heated-channel/channel /tmp/cht-20
# In /tmp/cht-20/system/fluid/blockMeshDict and /tmp/cht-20/system/solid/blockMeshDict, change "n 10;" to "n 20;".
# In /tmp/cht-20/system/controlDict, change endTime and writeInterval to 4000.
openfoam blockMesh -case /tmp/cht-20 -region fluid
openfoam blockMesh -case /tmp/cht-20 -region solid
openfoam chtMultiRegionSimpleFoam -case /tmp/cht-20
```

The values are in the last rows of the tables in `/tmp/cht-20/postProcessing/<region>`.

| n | Cells (fluid + solid) | Iterations | `interface_T` | `heater_T` | `outlet_T` |
|---:|---:|---:|---:|---:|---:|
| 5 | 1125 + 1125 | 3000 | 312.888716 | 313.368708 | 329.999850 |
| 10 | 4500 + 4500 | 3000 | 312.900027 | 313.380018 | 329.999800 |
| 20 | 18000 + 18000 | 4000 | 312.903008 | 313.383000 | 329.999757 |
| Extrapolated | | | 312.904075 | 313.384067 | |
| Exact | | | 312.904345 | 313.384345 | 330 |
| 10, the check | 4500 + 4500 | 2000 | 312.900027 | 313.380018 | 329.999800 |

The temperatures converge monotonically. The extrapolation from the meshes 5, 10 and 20 uses the observed order p, which is 1.92.
The extrapolated values are 0.00027 K below the exact values. Thus the mesh error of n = 10 is -0.0040 K in `interface_T` and in `heater_T`.
At n = 10 the Nusselt number from `interface_T` and the exact T_b is 8.251, 0.19 % above 140/17.

The mesh error comes from the cells across the channel. The fully developed profile on the same cells, solved exactly as a 1D finite volume model, has the errors -0.0154, -0.0040 and -0.0010 K at n = 5, 10 and 20. The errors of the case are 0.0002 to 0.0003 K lower than these.

The 0.0003 K between the extrapolated values and the exact values has these causes:

- Heat that leaves through the inlet by conduction: 0.0002 to 0.0003 K. `outlet_T` shows it, and the heat balance of the section at x0 shows the same deficit.
- The 2 ends of the heater: 7e-5 K (see "Model").
- The kinetic energy in `div(phi,K)`: from the uniform inlet profile to the parabola, the bulk kinetic energy increases by 0.011 J/kg. Thus T decreases by 1e-5 K.

The iteration error at 2000 iterations is below 1e-7 K: at n = 10, the values at 2000 and 3000 iterations differ by 1e-8 K or less. A finer mesh needs more iterations: n = 20 needs 3500.

On a Mac with 10 cores at a load of about 45, the case at n = 10 took 36 s for 3000 iterations.

## Tolerance

The tolerance is `rel_tol = 0` and `abs_tol = 0.006`, in K.

| Cause | `interface_T`, `heater_T` | `outlet_T` |
|---|---:|---:|
| Mesh error of n = 10, times 1.25 (the factor of safety of the grid convergence index) | 0.0051 K | 0.0001 K |
| The model against the exact solution | 0.0003 K | 0.0003 K |
| Iterations | below 1e-7 K | below 1e-7 K |

The largest sum is 0.0054 K, for `interface_T` and `heater_T`. `abs_tol` is this sum rounded up to 0.006 K.
The model is 0.0002 K to 0.0043 K below the exact values. The largest part of the tolerance that a check uses is 72 % (`heater_T`).
The tolerance does not let a mesh with n = 5 pass: there `interface_T` is 0.0156 K below the exact value.
