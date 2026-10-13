// femahoy fixture: a block of 4 x 1 x 1, quadratic tetrahedra (C3D10), unstructured.
// Make the mesh with: gmsh block-tet10.geo -3 -format msh41 -o block-tet10.msh
L = 4;
H = 1;
W = 1;
Point(1) = {0, 0, 0};
Point(2) = {L, 0, 0};
Point(3) = {L, H, 0};
Point(4) = {0, H, 0};
Line(1) = {1, 2};
Line(2) = {2, 3};
Line(3) = {3, 4};
Line(4) = {4, 1};
Curve Loop(1) = {1, 2, 3, 4};
Plane Surface(1) = {1};
// out[0] is the top surface, out[1] the volume, out[2] to out[5] the sides from lines 1 to 4.
out[] = Extrude {0, 0, W} { Surface{1}; };
Physical Surface("FIX") = {out[5]};
Physical Surface("TIP") = {out[3]};
Physical Volume("BLOCK") = {out[1]};
Mesh.MeshSizeMax = 0.5;
Mesh.ElementOrder = 2;
