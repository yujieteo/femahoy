# patch-test with C3D10

The geometry, material and analytical reference follow
[the original case](../patch-test/derivation.md).
The case retains all original tolerances and order bounds.

Each original hex gives 24 quadratic tetrahedra.
Each hex face has a shared centre node and 4 triangles.
Each triangle and the hex centre define a tetrahedron.
All tetrahedra share their midside nodes across common edges.
New node sets include nodes whose parent corners all belong to the original set.

All new boundary nodes receive the linear displacement field from their parent corners.
The N9 set selects the original interior node.
