# lame-cylinder with C3D10

The geometry, material and analytical reference follow
[the original case](../lame-cylinder/derivation.md).
The case retains all original tolerances and order bounds.

Each original hex gives 24 quadratic tetrahedra.
Each hex face has a shared centre node and 4 triangles.
Each triangle and the hex centre define a tetrahedron.
All tetrahedra share their midside nodes across common edges.
New node sets include nodes whose parent corners all belong to the original set.

The geometry retains straight chords at the inner and outer radii.
The inner tet faces receive 100 MPa through *DLOAD.
This replaces the nodal forces of the original hex fixture.
The symmetry and plane-strain constraints include all new boundary nodes.
