// A symmetric H-plane step between WR-90 (22.86 mm) and a guide of 16 mm, both 10.16 mm high.
// Lengths in metres. The wide guide is z = 0 to 0.025, the narrow guide z = 0.025 to 0.05.
SetFactory("OpenCASCADE");
DefineConstant[ h = 0.0005 ];
Box(1) = {0, 0, 0, 0.02286, 0.01016, 0.025};
Box(2) = {0.00343, 0, 0.025, 0.016, 0.01016, 0.025};
BooleanUnion(3) = { Volume{1}; Delete; }{ Volume{2}; Delete; };
e = 1e-6;
p1() = Surface In BoundingBox{-e, -e, -e, 0.02286 + e, 0.01016 + e, e};
p2() = Surface In BoundingBox{0.00343 - e, -e, 0.05 - e, 0.01943 + e, 0.01016 + e, 0.05 + e};
all() = Boundary{ Volume{3}; };
pec() = all();
pec() -= {p1(), p2()};
Physical Volume(1) = {3};
Physical Surface(2) = {p1()};
Physical Surface(3) = {p2()};
Physical Surface(4) = {pec()};
Mesh.CharacteristicLengthMax = h;
Mesh.CharacteristicLengthMin = h;
