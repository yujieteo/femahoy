# grid-coax-capacitor derivation

This case checks the electrostatic step of the built-in grid solver (`[step] type = "electrostatic"`): the capacitance matrix of 2 conductors in a dielectric, from the charge of each conductor (the residual of the rows of its points). It is not a package problem. Units are SI-mm.

## Source

A coaxial capacitor of length L, with the inner radius a, the outer radius b and a dielectric of relative permittivity εr, has C = 2π ε0 εr L / ln(b / a) when the field is radial. The 2 end faces have no charge, so the field is radial. P. P. Silvester and R. L. Ferrari, *Finite Elements for Electrical Engineers*, 3rd ed., Cambridge, 1996: the electrostatic potential on finite elements, and the capacitance from the charge.

## Model

| Item | Value |
|---|---|
| Capacitor | a = 1 mm, b = 2.3 mm, L = 20 mm along z |
| Dielectric | PTFE, εr = 2.1 |
| Conductors | the hole (`inner`, 1 V) and the outer face (`outer`, 0 V) of the dielectric part |

ε0 = 8.8541878128e-12 F/m (CODATA 2018). C = 2π × 8.8541878128e-12 × 2.1 × 0.02 / ln(2.3) = 2.8053061878e-12 F. The check is the diagonal entry `inner.inner` of the Maxwell capacitance matrix; the entry `inner.outer` is −C.

## Mesh study

| max_cell (mm) | length (mm) | points | C (F) | Error |
|---:|---:|---:|---:|---:|
| 1.0 | 20 | 11,445 | 2.8128574e-12 | +0.269 % |
| 0.5 | 20 | 22,345 | 2.8128794e-12 | +0.270 % |
| 0.3 | 20 | 38,828 | 2.8122253e-12 | +0.247 % |
| 0.1 | 0.2 | 8,538 | 2.8046691e-14 | −0.023 % |
| 0.05 | 0.2 | 55,960 | 2.8051495e-14 | −0.0056 % |
| 0.035 | 0.2 | 159,334 | 2.8052185e-14 | −0.0031 % |

The field does not change along z, so the error comes from the triangles of the cross-section. Above 0.1 mm, the 64 segments of each circle set the size of the triangles, and the error stays at +0.27 %. Below it, the error falls by a factor of 4.1 when the cell halves (second order). The rows of length 0.2 mm keep these small cells to a small mesh; their C is per 0.2 mm (1/100 of the exact value above). The check uses max_cell 0.5 mm (0.4 s). Its error is +0.27 %. The tolerance is 0.5 %.
