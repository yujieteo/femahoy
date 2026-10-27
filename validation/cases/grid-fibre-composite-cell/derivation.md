# grid-fibre-composite-cell derivation

This case checks the homogenized material of 2 periodic unit cells of a fibre composite: carbon fibres along z in epoxy. The round fibres check the periodic tetrahedral mesh and the conductivity across the fibres against a published formula. The square fibres check the stiffness of the box grid along the fibres against an exact answer.

## Model

| Item | round (SI-m) | square (SI-mm) |
|---|---|---|
| Cell | 1 x 1 x 0.05 | 1 x 1 x 0.5 |
| Fibre | circle at (0.5, 0.5), radius sqrt(0.5/π), f = 0.5 | square 0.2 to 0.8 in x and y, f = 0.36 |
| Epoxy | k = 0.2 | k = 0.2, E = 3500 MPa, ν = 0.3, ρ = 1.2e-9 |
| Carbon | k = 10 | k = 10, E = 230000 MPa, ν = 0.3, ρ = 1.8e-9 |
| Mesh | tetrahedra, max_cell 0.02, max_growth 1.2 | box grid, max_cell 0.05 |

## Reference

Round fibres, across the fibres. Lord Rayleigh, On the influence of obstacles arranged in rectangular order upon the properties of a medium, Phil. Mag. 34 (1892) 481-502, solved the square array of cylinders. W. T. Perrins, D. R. McKenzie and R. C. McPhedran, Transport properties of regular arrays of cylinders, Proc. R. Soc. Lond. A 369 (1979) 207-225, give the lattice sums of its formula:

k*/km = 1 + 2 f ρ / (1 − f ρ − 0.305827 ρ² f⁴ / (1 − 1.402958 ρ² f⁸) − 0.013362 ρ² f⁸), with ρ = (kf − km) / (kf + km).

With km = 0.2, kf = 10 and f = 0.5: ρ = 0.960784, and k* = 0.5829156886.

The Hashin-Shtrikman bounds of a transversely isotropic 2D composite (Z. Hashin and S. Shtrikman, J. Appl. Phys. 33 (1962) 3125-3131) are km + f / (1/(kf − km) + (1 − f)/(2 km)) = 0.569811 and kf + (1 − f) / (1/(km − kf) + f/(2 kf)) = 3.509934. The reference and the result are inside them, near the lower bound, because the matrix is continuous.

Round fibres, along the fibres: the field is uniform, and kzz = km + f (kf − km). The polygon of the circle has the volume fraction f = 0.4997928030 at max_cell 0.02, so kzz = 5.0979694697.

Square fibres, along the fibres. With the same Poisson ratio in the 2 materials, a uniform strain (εzz = 1, εxx = εyy = −ν) is in equilibrium and is compatible, so Ez = f Ef + (1 − f) Em = 85040 MPa exactly (R. Hill, J. Mech. Phys. Solids 12 (1964) 199-212). In the same way kzz = 3.728 and the density 1.416e-9 are the means.

## Mesh study

Round fibres:

| max_cell | points | kxx | error against Rayleigh |
|---:|---:|---:|---:|
| 0.04 | 4,095 | 0.582461 | −0.078 % |
| 0.02 | 21,032 | 0.582811 | −0.018 % |
| 0.01 | 124,626 | 0.582891 | −0.004 % |

The square fibres give Ez = 85040.0000 and kzz = 3.72800000 at max_cell 0.05.

## Tolerance

The change of kxx from max_cell 0.02 to 0.01 is 0.014 %, so the discretization error at 0.02, with the area of the polygon of the circle, is about 0.02 %. The formula leaves out the terms of f¹² and above, about 0.01 % at f = 0.5. Their sum is 0.03 %, and the tolerance is 0.1 %. The checks along the fibres are exact to 1e-10, inside the same tolerance.
