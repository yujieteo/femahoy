# Cantilever: derivation

Input meshes: `fixtures/inp/cantilever.inp` (4 elements), `mesh-8.inp`, `mesh-16.inp` (same geometry, 8 and 16 elements along x, 1 element across).
Mesh ratio 2. The 2 finer files are our own output of a script that halves the element length.

## Data

    L = 10 mm, b = 1 mm (y), h = 1 mm (z), A = b h = 1 mm^2
    E = 210000 MPa, nu = 0.3
    P = 4 * 0.25 N = 1.0 N in -z at the 4 tip nodes (fixed end at x = 0)
    I = b h^3 / 12 = 1/12 mm^4
    G = E / (2 (1 + nu)) = 80769.23 MPa
    k = 5/6 (shear factor, rectangular section)

## Reference

Bending (Euler-Bernoulli):

    d_b = P L^3 / (3 E I) = 1.0 * 1000 / (3 * 210000 / 12) = 1000 / 52500 = 0.019047619 mm

Shear (Timoshenko):

    d_s = P L / (k G A) = 10 / (0.83333 * 80769.23) = 1.485714e-4 mm

Total:

    d = d_b + d_s = 0.019047619 + 0.000148571 = 0.019196190 mm in -z

The reference value in `case.kv` is -0.01919619048 mm (z is the load direction).

## Reference error

Beam theory ignores the 3D stress state at the clamp. The fixture sets 2 percent of the reference as the stated error. This is an engineering estimate, not a derived bound. It needs captain confirmation.
