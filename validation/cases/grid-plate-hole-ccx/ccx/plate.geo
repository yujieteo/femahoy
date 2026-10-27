// The geometry of the CalculiX reference of grid-plate-hole-ccx: a steel plate with a round
// hole and a copper pad on its top. The 2 volumes share the nodes of their common face.
//   gmsh plate.geo -3 -setnumber H 0.002 -o mesh.inp
SetFactory("OpenCASCADE");
DefineConstant[ H = 0.002 ];
Box(1) = {0, 0, 0, 0.1, 0.06, 0.01};
Cylinder(2) = {0.03, 0.03, 0, 0, 0, 0.01, 0.01};
BooleanDifference(3) = { Volume{1}; Delete; }{ Volume{2}; Delete; };
Box(4) = {0.06, 0.02, 0.01, 0.03, 0.02, 0.01};
v() = BooleanFragments{ Volume{3, 4}; Delete; }{};
Physical Volume("PLATE") = {Volume In BoundingBox{-1e-6, -1e-6, -1e-6, 0.1 + 1e-6, 0.06 + 1e-6, 0.01 + 1e-6}};
Physical Volume("PAD") = {Volume In BoundingBox{0.06 - 1e-6, 0.02 - 1e-6, 0.01 - 1e-6, 0.09 + 1e-6, 0.04 + 1e-6, 0.02 + 1e-6}};
Mesh.MeshSizeMax = H;
Mesh.MeshSizeMin = H / 4;
Mesh.ElementOrder = 2;
Mesh.SecondOrderLinear = 0;
Mesh.SaveAll = 0;
