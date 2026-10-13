# cantilever with C3D10

The geometry, material and analytical reference follow
[the original case](../cantilever/derivation.md).
The case retains all original tolerances and order bounds.

Each original hex gives 24 quadratic tetrahedra.
Each hex face has a shared centre node and 4 triangles.
Each triangle and the hex centre define a tetrahedron.
All tetrahedra share their midside nodes across common edges.
New node sets include nodes whose parent corners all belong to the original set.

All new clamp nodes join the fixed set.
The 1 N total load stays on the 4 original tip corners.
The tip set includes new nodes, so the measurement is the mean U3 over the tip face.
