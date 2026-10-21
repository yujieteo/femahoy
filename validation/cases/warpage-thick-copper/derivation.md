# Measured warpage of a wafer slice with thick copper

The reference is a measured value, not a theory.
Source: M. Calabretta, A. Sitta, S. M. Oliveri and G. Sequenzia, *Warpage Behavior on Silicon Semiconductor Device: The Impact of Thick Copper Metallization*, Applied Sciences 11 (2021) 5140.
The paper is open access under CC BY 4.0: <https://doi.org/10.3390/app11115140>.
The numbers below come from its sections 2 and 4.2 and its Tables 1 and 2.

## The part and the measurement

The part is a slice of a power semiconductor wafer from STMicroelectronics, cut after the copper deposition.
The slice is 50 x 10 x 0.75 mm. The layers are 730 µm of silicon, 0.3 µm of TiW, 0.2 µm of seed copper and 20 µm of electrochemically deposited (ECD) copper.

The paper measured the warpage with phase-shift projection moiré in an Insidix TDM Compact.
The warpage is the maximum deflection along a cutline of each warpage map.
In the first thermal cycle, the copper grains grow and the warpage changes permanently.
In the second cycle, the warpage from 50 to 90 °C is linear in temperature.
Table 2 gives the measured warpage rate in this range: 0.700 µm/°C.

## Inputs

The inputs are the values of the paper (Table 1, section 4.2). The case changes none of them.

| Body | z (mm) | E (MPa) | nu | alpha (1/K) |
|---|---|---:|---:|---:|
| SI | 0 to 0.73 | 120000 | 0.28 | 2.5e-6 |
| CU | 0.73 to 0.75 | 77500 | 0.364 | 1.64e-5 |

The paper models the same 2 layers. It leaves out the TiW layer and the seed copper layer because they are thin.
Both materials are linear elastic and isotropic, with properties that do not change with temperature, as in the paper.
The temperature change is 40 K, the range of the measured rate.
The input starts at 0 and ends at 40. For a linear model with constant properties, only the change has an effect.

The quarter model uses x = 0 to 25, y = 0 to 5 and z = 0 to 0.75 mm.
Symmetry gives u1 = 0 at x = 0 and u2 = 0 at y = 0. The node `ORIGIN` at (0, 0, 0) has u3 = 0.
The node `END` is at (25, 0, 0), at the end of the centre cutline on the silicon face.
Thus u3 at `END` is the deflection of the end relative to the centre, which is the warpage over the 50 mm length.

## Reference

The reference is -0.700 µm/K x 40 K = -0.028 mm.
Table 2 gives the magnitude. The sign follows from the model axes.
When the temperature increases, the copper expands more than the silicon. Thus the copper face becomes convex and the end moves to -z.

## Tolerance

The relative tolerance is 0.12. It is the root sum square of these contributions, rounded up from 11.2%:

| Contribution | Basis | Effect |
|---|---|---:|
| Moiré accuracy | ±1.5 µm for each map (distributor data for the TDM Compact 3; the A*STAR equipment page gives < 1 µm or 2%). 2 maps give the change of 28.0 µm. | 7.6% |
| Temperature | The thermocouple is on the stage, not on the slice. The paper gives no uncertainty. The budget uses ±1 K at 50 °C and at 90 °C. | 3.5% |
| Layers not in the model | The seed copper (0.2 µm) gives +0.9% in plate theory. The paper gives no TiW data. The budget gives the TiW layer the effect of 0.3 µm of copper. | 2.4% |
| Silicon elasticity | The inputs give a biaxial modulus E/(1 - nu) of 166.7 GPa. The (100) silicon wafer has 180.5 GPa (Hopcroft, Nix and Kenny, J. MEMS 19 (2010) 229). | 7.1% |

## Check of the solve

For the same inputs, the plate form of the bilayer curvature (Timoshenko) gives -0.0271253 mm (0.67813 µm/K).
It uses the plate moduli E/(1 - nu) of each layer.
A free bilayer plate in linear theory bends to a spherical shape, so the plate moduli apply away from the free edges.
The beam form with E gives 0.60445 µm/K. The paper gives 0.605 µm/K for it.
`cargo test -p fh --test cli` checks that the warpage of `fh validate` is within 1% of the plate value.

## Measured result

The check passed on macOS 27.0.1 arm64 with Python 3.12.13, build123d 0.13.0, gmsh 4.15.2-git and CalculiX 2.23.
The solver time is the run time that `fh run` prints. It changes with the load of the machine.

| Mesh size (mm) | Nodes | C3D10 elements | Shared interface nodes | Minimum scaled Jacobian | Warpage (mm) | Rate (µm/K) | Measured (mm) | Difference (%) | Solver time (s) |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1.0 | 3790 | 2065 | 677 | 0.0008 | -0.0271124 | 0.67781 | -0.028 | -3.17 | 0.4377 |
| 0.5 | 13334 | 7583 | 2489 | 0.0005 | -0.0271044 | 0.67761 | -0.028 | -3.20 | 1.388 |
| 0.25 | 72959 | 45949 | 9653 | 0.0003 | -0.0270953 | 0.67738 | -0.028 | -3.23 | 12.53 |

The computed warpage is 3.2% below the measured value. This is within the 12% tolerance.
The 3 mesh sizes differ by 0.063%. The finest value is 0.11% below the plate value.
The paper's own linear model gives 0.660 µm/K, 5.7% below the measured value. The femahoy value is 2.6% above it.

The minimum scaled Jacobian is low, because `fh mesh` has one size for the full part.
Thus the 20 µm copper layer gets flat tetrahedra. All Jacobians are positive, and the warpage changes by only 0.063% between the 3 mesh sizes.

## Causes of the difference

1. The silicon modulus of the paper is low for a (100) wafer. The (100) value gives 7.1% less warpage, and a difference of about -10%.
2. The model leaves out the seed copper and the TiW layer. The seed copper alone adds 0.9%.
3. The moiré accuracy is about 7.6% of the measured change.
4. The copper modulus comes from nanoindentation. The paper gives no uncertainty for it. The warpage is almost proportional to it.
5. The paper does not give the mesh of its own model. Linear hexahedral elements are stiff in bending, which can give a lower value than C3D10.

## Reproduce

Make a mesh with H = 1, 0.5 or 0.25, and replace the mesh in a copy of `job.inp`. The mesh in `job.inp` is the coarse mesh from `fh mesh slice.step --size 1 --volume SI=box:0,0,0,25,5,0.73 --volume CU=box:0,0,0.73,25,5,0.75 --face SYM_X=x:0 --face SYM_Y=y:0 --nodes ORIGIN=point:0,0,0 --nodes END=point:25,0,0`. The STEP file comes from `scripts/cad/thick_copper_slice.py`.

## Choice of the case

The case must be a real part, with open data for the geometry, the materials, the temperature change and the measured warpage.
A linear thermoelastic model must be able to predict it.

| Candidate | Result |
|---|---|
| Calabretta et al. 2021, wafer slice with 20 µm copper | Selected. The paper gives all inputs and a measured rate in a range that it shows to be linear. |
| Farshchi Yazdi et al., Micromachines 14 (2023) 165, glass frit wafer bonding | Not selected. The measured warpage is only in a figure. The paper gives no frit expansion and uses cohesive elements and clamps. |
| Thermostatic bimetal data sheets (Aperam, Kanthal) | Not selected. They give a measured specific curvature, but not the layer thicknesses or the data of each layer. |
| Silicon bonded anodically to Borofloat 33 glass | Not selected. Only informal bow values are open. The expansion mismatch is small and changes with temperature. |
| Molded and flip-chip packages with shadow moiré | Not selected. Mold compound and underfill are viscoelastic and shrink during cure. |
| Circuit boards with shadow moiré | Not selected. FR-4 is orthotropic and viscoelastic near its glass transition. |

## Limits

This case checks only the linear range of the second cycle.
The first cycle needs copper plasticity and the grain growth, which are nonlinear models. They are follow-up work.
