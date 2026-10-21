# acoustics-pulsating-sphere derivation

This case checks the acoustic path: the sound that a pulsating sphere radiates into air at 500 Hz.
The reference is the analytical solution. Thus the tolerance comes only from the mesh error of the model.

## Why this case

No other case of the suite has any of these items:

- Acoustics: the Helmholtz equation for the sound pressure, solved with GetDP.
- Radiation into an unbounded region, with an absorbing condition on the outer face.
- Second-order node functions (`BF_Node_2E`) on a first-order mesh.
- A check of a mean over a surface (the radiation impedance) and of a complex value at a point.

## Sources

- L. E. Kinsler, A. R. Frey, A. B. Coppens and J. V. Sanders, "Fundamentals of Acoustics", 4th edition, Wiley, 2000, ISBN 0-471-84789-5, section 7.1, "Radiation from a pulsating sphere".
  It gives the outgoing spherical wave of a sphere with a uniform normal velocity, and its radiation impedance.

The derivation below gives the same result, so that a reader can check it without the book.
With the time factor exp(j omega t), the outgoing spherical wave is p = A exp(-j k r) / r, with k = omega / c.
The linear Euler equation gives the radial velocity: j omega rho u = -dp/dr.
On the sphere r = a, u = U. Thus:

```
p(r) = rho c U z (a / r) exp(-j k (r - a))
z = j ka / (1 + j ka) = (ka)^2 / (1 + (ka)^2) + j ka / (1 + (ka)^2)
```

z is the specific radiation impedance of the sphere: p on the sphere divided by rho c U.
The same wave also gives dp/dr = -(j k + 1 / r) p at each r. This is the condition on the outer face of the model.

| Item | Value |
|---|---|
| Sphere | radius a = 0.1 m, normal velocity U = 1e-3 m/s, uniform |
| Air | c = 343 m/s, rho = 1.2 kg/m3 |
| Frequency | 500 Hz: k = 9.159162 1/m, ka = 0.915916, wavelength 0.686 m |
| rho c U | 0.4116 Pa |

| Check | Quantity | Value |
|---|---|---:|
| `z_re` | Re(z), the radiation resistance | 0.456197 |
| `z_im` | Im(z), the radiation reactance | 0.498078 |
| `p_re` | Re(p) in Pa at (0.2, 0.2, 0.1), r = 0.3 m | 4.98678e-2 |
| `p_im` | Im(p) in Pa at the same point | -7.81063e-2 |

At r = 0.3 m, the wave has gone 0.2 m from the sphere: 0.29 wavelength. The phase of p there is -1.003 rad, thus both parts are large.

## Model

The GetDP case is the folder `500hz`: `case.geo` for gmsh and `case.pro` for GetDP. The units are SI, with lengths in m.

| Item | Value |
|---|---|
| Region | the air between the sphere r = a and the outer sphere r = R = 0.5 m, in the octant x, y, z >= 0 |
| Equation | div grad p + k^2 p = 0 in the air |
| Symmetry planes | dn p = 0, the natural condition. It is a rigid wall, and the solution is symmetric about each plane |
| Sphere | dn p = j omega rho U, with the normal n into the sphere |
| Outer sphere | dn p = -(j k + 1 / R) p. It is exact for the outgoing spherical wave, thus it reflects no part of the wave |
| Elements | first-order tetrahedra. The pressure has second-order node functions: `BF_Node` on the nodes and `BF_Node_2E` on the edges. Gauss rules of 15 points (tetrahedra) and 7 points (triangles) |
| Mesh | size 10 mm at the sphere and 50 mm at the outer sphere: 13.7 elements in a wavelength. 2248 nodes, 9527 tetrahedra, 15356 unknowns |
| Output | `z.txt`: 1 row with the 4 values of the check. z is the integral of p on the sphere, divided by the area of the meshed sphere and by rho c U. `p.pos` and `p_abs.pos`: p and \|p\| in the air, for the viewer |

The .pos files have first-order elements, because the mesh is first-order. Thus `fh run` reads them, and the page shows them.

## Mesh study

These commands, in the repository root, solve the case on another mesh in a scratch folder. `-setnumber` sets the mesh sizes of `case.geo` in m:

```
mkdir /tmp/sphere && cp validation/cases/acoustics-pulsating-sphere/500hz/case.* /tmp/sphere && cd /tmp/sphere
gmsh case.geo -3 -o case.msh -setnumber ha 0.007 -setnumber hR 0.035
getdp case.pro -msh case.msh -solve '#1' -pos '#1'
```

The values are in `/tmp/sphere/z.txt`. The 5 meshes use the sizes of the case times 2, 1.5, 1, 0.7 and 0.5.

| Mesh | Unknowns | Time | z_re | z_im | p_re | p_im |
|---|---:|---:|---:|---:|---:|---:|
| x 2 | 3041 | 0.1 s | 0.454279 | 0.497545 | 4.93831e-2 | -7.77939e-2 |
| x 1.5 | 5822 | 0.2 s | 0.455130 | 0.497872 | 4.95266e-2 | -7.78776e-2 |
| x 1, the check | 15356 | 0.6 s | 0.455698 | 0.498019 | 4.97726e-2 | -7.80087e-2 |
| x 0.7 | 38027 | 2.1 s | 0.455953 | 0.498056 | 4.98037e-2 | -7.80739e-2 |
| x 0.5 | 98252 | 8.0 s | 0.456070 | 0.498070 | 4.98382e-2 | -7.80862e-2 |
| Analytical | | | 0.456197 | 0.498078 | 4.98678e-2 | -7.81063e-2 |

The times are the times of GetDP 4.0.0 on a Mac with 10 cores.

| Mesh | Error of z_re | Error of z_im | Error of p_re | Error of p_im |
|---|---:|---:|---:|---:|
| x 2 | -0.42 % | -0.11 % | -0.97 % | -0.40 % |
| x 1.5 | -0.24 % | -0.04 % | -0.68 % | -0.29 % |
| x 1, the check | -0.11 % | -0.01 % | -0.19 % | -0.13 % |
| x 0.7 | -0.06 % | 0.00 % | -0.13 % | -0.04 % |
| x 0.5 | -0.03 % | 0.00 % | -0.06 % | -0.03 % |

Each value goes to the analytical value as the mesh becomes finer. The error of z_re falls by a factor of about 2 for each step: the meshed sphere is a polyhedron, and its error falls as h^2.
The meshed sphere of the check mesh has 0.19 % less area than the octant of the sphere. z uses the area of the meshed sphere.

## Tolerance

The tolerance is `rel_tol = 0.01` and `abs_tol = 0`.

| Cause | Basis | Effect |
|---|---|---:|
| The reference | exact | 0 |
| Mesh error of the check mesh | the largest error of the table above, `p_re` | 0.19 % |
| Another mesh of the same sizes | gmsh of another version can make another mesh. Its error is near the error of the check mesh. The coarser mesh x 1.5 has 0.68 % | 0.68 % or less |

`rel_tol` = 1 % covers both. No check value is near 0, thus `abs_tol` is 0.

| Check | Model | Analytical | Difference | Tolerance | Part of the tolerance |
|---|---:|---:|---:|---:|---:|
| `z_re` | 0.455698 | 0.456197 | -4.99e-4 | 4.56e-3 | 11 % |
| `z_im` | 0.498019 | 0.498078 | -5.9e-5 | 4.98e-3 | 1 % |
| `p_re` | 4.97726e-2 | 4.98678e-2 | -9.52e-5 | 4.99e-4 | 19 % |
| `p_im` | -7.80087e-2 | -7.81063e-2 | 9.76e-5 | 7.81e-4 | 12 % |

The tolerance stops these errors. Each line is a change of `case.pro` on the check mesh:

| Wrong model | z_re | z_im | p_re | p_im | Largest error |
|---|---:|---:|---:|---:|---:|
| Outer condition dn p = -j k p, without 1 / R | 0.497653 | 0.402873 | 5.43568e-2 | -8.84014e-2 | 19 % |
| Outer condition for the time factor exp(-j omega t) | -0.455698 | 0.498019 | -4.97726e-2 | -7.80087e-2 | 200 % |
| Rigid outer sphere (no outer condition) | 0 | 5.19191 | 0 | 0.434733 | 940 % |
| First-order node functions only | 0.445171 | 0.495703 | 4.97326e-2 | -7.55207e-2 | 3.3 % |
| Sign of the velocity on the sphere | -0.455698 | -0.498019 | -4.97726e-2 | 7.80087e-2 | 200 % |
| c = 340 m/s, not 343 m/s | 0.460047 | 0.498394 | 4.79655e-2 | -7.86958e-2 | 3.8 % |

## Not in the check

- Other frequencies: the solution is the same function of ka at each frequency. 1 frequency with ka near 1 checks the resistance and the reactance with the same weight.
- Structural modes or a structure that vibrates in air: CalculiX solves the modes of the structure. A coupled case needs the velocity of the structure on the surface, which this case gives as U.

On a Mac with 10 cores, `fh run` of the case took 1.7 s. gmsh took 0.1 s of it.
