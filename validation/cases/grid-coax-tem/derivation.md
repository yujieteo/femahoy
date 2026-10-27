# grid-coax-tem derivation

This case checks the TEM port of the built-in electromagnetic solver: a coaxial line with a TEM port at each end. The port solves 2 electrostatic problems on its face to get the impedance and the propagation constant of the line (`fh guide grid`). The case is not a chip package problem. Units are SI-mm.

## Source

A coaxial line with the inner radius a, the outer radius b and a dielectric of εr has the TEM mode with Z0 = η0 ln(b / a) / (2π sqrt(εr)) and β = k0 sqrt(εr). A uniform line with matched ports has S21 = exp(−jβL). D. M. Pozar, *Microwave Engineering*, 4th ed., Wiley, 2012, sections 2.1 and 3.4.

## Model

| Item | Value |
|---|---|
| Line | a = 1 mm, b = 2.3 mm, PTFE εr = 2.1, L = 20 mm along z |
| Conductors | the hole and the outer face of the dielectric part, perfect conductors (the default wall) |
| Ports | TEM, signal on the inner face, at z = 0 (port 1) and z = 20 mm (port 2) |
| Frequency | 5 GHz |

η0 = 376.730313 ohm. Z0 = 376.730313 × ln(2.3) / (2π × 1.449138) = 34.461857 ohm. β = 104.719755 × 1.449138 = 151.858399 rad/m, βL = 3.037168 rad, so the phase of S21 is −174.016907 degrees.

| Check | Formula | Value |
|---|---|---|
| `impedance` | η0 ln(b / a) / (2π sqrt(εr)) | 34.461857 ohm |
| `s21_mag` | \|exp(−jβL)\| | 1 |
| `s21_deg` | −βL in degrees | −174.016907 |

## Mesh study

| max_cell (mm) | points | unknowns | impedance (ohm) | \|S21\| | phase of S21 (degrees) | time |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 2,058 | 9,832 | 33.819044 | 0.9999891 | −174.30194 | 0.10 s |
| 0.5 | 5,289 | 26,407 | 34.371751 | 0.9999983 | −174.29059 | 0.43 s |
| 0.25 | 37,179 | 220,227 | 34.495101 | 0.9999998 | −174.09733 | 27 s |

The circles are polygons with their points on the circle, so the inner conductor is a little too small and the outer conductor too. The 2 effects have opposite signs in the impedance, so its error changes sign between 0.5 and 0.25 mm. \|S11\| is below 3e-4 on each mesh.

## Tolerance

The check uses max_cell 0.5 mm. Its largest relative error is −2.6e-3 (the impedance). The tolerance is 5e-3 of each value.
