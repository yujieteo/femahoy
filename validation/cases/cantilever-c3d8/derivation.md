# C3D8 cantilever

The geometry, material, 4 corner loads and measured quantity match
[the original cantilever](../cantilever/derivation.md).
The reference remains -0.01919619048 mm.
The reference error remains 2 percent.
The order band remains 1.0 to 3.0.

Each section has 4 elements along y and 4 along z.
The axial meshes have 8, 16, 32 and 64 elements.
The fixed end constrains all nodes at x = 0.
The 4 corner nodes at x = 10 each receive -0.25 N in z.
The tip set contains only these 4 nodes.
The measured quantity is their mean U3.

Richardson uses the 3 finest axial meshes with ratio 2.
The observed order is 1.78.
The extrapolated value is -0.019470567508 mm.
Its difference from beam theory is 1.43 percent.
Section discretisation remains fixed, so this case checks axial convergence only.

C3D8 with 1 element through the section is too soft in bending.
Axial refinement alone does not remove that error.
This fixture uses 4 elements through each section axis to reduce that effect.
