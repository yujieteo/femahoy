# Conjugate heat transfer with OpenFOAM

For heat that goes between a solid and a flow, write an OpenFOAM case with 1 region for each fluid and each solid, and solve it with `fh run CASE`.

1. Regions: `constant/regionProperties` with `regions (fluid (fluid) solid (solid));`. For each region R: `system/R/blockMeshDict`, `system/R/fvSchemes`, `system/R/fvSolution`, `constant/R/thermophysicalProperties` and the start fields in `0/R`. Also write `constant/g`, and `system/fvSchemes` and `system/fvSolution` with empty contents.
2. Interface: in each `blockMeshDict`, the interface patch has `type mappedWall`, `sampleMode nearestPatchFace`, `sampleRegion` (the other region) and `samplePatch` (its patch). Give the 2 meshes the same faces on the interface. On it, T is `compressible::turbulentTemperatureRadCoupledMixed` with `Tnbr T` and `kappaMethod fluidThermo` in the fluid, `solidThermo` in the solid.
3. Solver: `chtMultiRegionSimpleFoam` for a steady state, `chtMultiRegionFoam` for a transient. The fluid is `heRhoThermo`, the solid is `heSolidThermo`. A heat flux on an outer face is `externalWallHeatFluxTemperature` with `mode flux` and `q`.
4. `fh run` starts `blockMesh -region R` for each region, the solver, then `foamToVTK -allRegions`. The page shows the cells of all regions in 1 mesh. A region with no field of a name, for example `U` in a solid, shows 0.
5. Numbers: add `region R;` to each function object. Its table is in `postProcessing/R/<name>/0`. A `surfaceFieldValue` with `operation weightedAverage` and `weightField phi` on the outlet gives the bulk temperature. Compare it with the energy balance.

Give `p_rgh` an absolute `tolerance` of about 1e-7. With 1e-12, GAMG does 1000 sweeps in each iteration, because `p_rgh` is near 1e5 Pa.
This setup agrees with the exact solution for laminar flow between 2 heated walls of finite thickness (the validation case `cht-heated-channel`): uniform cells, `bounded Gauss linear` for `div(phi,U)`, `div(phi,h)` and `div(phi,K)`, the relaxation factors 0.7 for `U` and `p_rgh` and 0.9 for `h` in the fluid, 1 for `h` in the solid, and 2000 iterations.
