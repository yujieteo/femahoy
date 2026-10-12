# Patch test: derivation

Source of the input: `fixtures/inp/patch-test.inp` (7 C3D8 elements, E = 210000 MPa, nu = 0.25).

## Displacement field

The 8 outer nodes carry the displacement of a linear field u = c + G x with c = 0:

    u = 0.0010 x + 0.0002 y + 0.0001 z
    v = 0.0003 x + 0.0005 y + 0.0001 z
    w = 0.0002 x + 0.0001 y + 0.0004 z

All 8 prescribed values in the `.inp` file satisfy these 3 equations. Example: node 7 at (1, 1, 1) has u = 0.0013, v = 0.0009, w = 0.0007.

Interior node 9 at (0.25, 0.25, 0.25):

    u = 0.25 (0.0010 + 0.0002 + 0.0001) = 0.000325
    v = 0.25 (0.0003 + 0.0005 + 0.0001) = 0.000225
    w = 0.25 (0.0002 + 0.0001 + 0.0004) = 0.000175

## Strain

    e_xx = 0.0010   e_yy = 0.0005   e_zz = 0.0004
    g_xy = 0.0002 + 0.0003 = 0.0005
    g_yz = 0.0001 + 0.0001 = 0.0002
    g_zx = 0.0001 + 0.0002 = 0.0003

## Stress (isotropic Hooke law)

    lambda = E nu / ((1 + nu)(1 - 2 nu)) = 210000 * 0.25 / (1.25 * 0.5) = 84000 MPa
    G      = E / (2 (1 + nu))             = 210000 / 2.5               = 84000 MPa
    trace  = 0.0010 + 0.0005 + 0.0004 = 0.0019

    s_xx = lambda trace + 2 G e_xx = 159.6 + 168.0 = 327.6 MPa
    s_yy = 159.6 + 2 * 84000 * 0.0005             = 243.6 MPa
    s_zz = 159.6 + 2 * 84000 * 0.0004             = 226.8 MPa
    s_xy = G g_xy = 84000 * 0.0005                =  42.0 MPa
    s_yz = 84000 * 0.0002                         =  16.8 MPa
    s_zx = 84000 * 0.0003                         =  25.2 MPa

The stress is the same in all 7 elements.
