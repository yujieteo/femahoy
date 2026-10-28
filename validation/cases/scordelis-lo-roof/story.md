## Conclusion

The S8R shell model gives the deflection of the middle of the free edge within 0.16 % of the published value. The model gives uz = -301.92 mm at TIP, and MacNeal and Harder (1985) give -302.4 mm. The difference is 0.48 mm, 16 % of the tolerance of 1 %.

<!--curve mesh.csv-->

The run also solves a second roof, the former S6 mesh of `fh mesh --shell`, 40000 mm away in y. It gives uz = -301.54 mm at TIP_S6, 0.3 % from the published value. This checks the S6 shell path (see "S6 check of the shell path").

The mapped mesh is converged. The 16 x 16 mesh gives -0.15 %, and the 32 x 32 mesh gives -0.16 % (see "Mesh convergence"). The case mesh of 24 x 24 is 0.001 % from the 32 x 32 mesh.
