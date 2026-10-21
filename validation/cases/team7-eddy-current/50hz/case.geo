// TEAM problem 7: an aluminium plate with a hole under a coil. Lengths in metres.
SetFactory("OpenCASCADE");
// Mesh sizes: plate, coil, the band around the probe line A1-B1, the outer sphere.
DefineConstant[ hp = 0.005, hc = 0.01, hb = 0.003, hs = 0.05, r1 = 0.35, r2 = 0.45 ];
Box(1) = {0, 0, 0, 0.294, 0.294, 0.019};
Box(2) = {0.018, 0.018, 0, 0.108, 0.108, 0.019};
BooleanDifference(3) = { Volume{1}; Delete; }{ Volume{2}; Delete; };
// The coil: the ring between a square of 0.200 with corner radius 0.050 and a square of 0.150
// with corner radius 0.025, both centred on (0.194, 0.100), from z = 0.049 to 0.149.
outer = news;
Rectangle(outer) = {0.094, 0, 0.049, 0.200, 0.200, 0.050};
inner = news;
Rectangle(inner) = {0.119, 0.025, 0.049, 0.150, 0.150, 0.025};
ring() = BooleanDifference{ Surface{outer}; Delete; }{ Surface{inner}; Delete; };
coil() = Extrude {0, 0, 0.100} { Surface{ring(0)}; };
// A band of fine mesh around the line A1-B1 (y = 0.072, z = 0.034).
Box(20) = {-0.006, 0.062, 0.026, 0.300, 0.020, 0.016};
// Air in the sphere r1, and the shell from r1 to r2 that GetDP maps to infinity.
Sphere(30) = {0.147, 0.147, 0.074, r1};
Sphere(31) = {0.147, 0.147, 0.074, r2};
BooleanFragments{ Volume{31}; Delete; }{ Volume{30, 3, coil[1], 20}; Delete; }
e = 1e-4;
plate() = Volume In BoundingBox{-e, -e, -e, 0.294 + e, 0.294 + e, 0.019 + e};
coil() = Volume In BoundingBox{0.094 - e, -e, 0.049 - e, 0.294 + e, 0.200 + e, 0.149 + e};
band() = Volume In BoundingBox{-0.006 - e, 0.062 - e, 0.026 - e, 0.294 + e, 0.082 + e, 0.042 + e};
inside() = Volume In BoundingBox{0.147 - r1 - e, 0.147 - r1 - e, 0.074 - r1 - e, 0.147 + r1 + e, 0.147 + r1 + e, 0.074 + r1 + e};
shell() = Volume{:};
shell() -= inside();
air() = inside();
air() -= {plate(), coil(), band()};
Physical Volume("plate", 1) = plate();
Physical Volume("coil", 2) = coil();
Physical Volume("air", 3) = air();
Physical Volume("band", 6) = band();
Physical Volume("shell", 7) = shell();
Physical Surface("outer", 4) = CombinedBoundary{ Volume{:}; };
Physical Surface("skin", 5) = Boundary{ Volume{plate()}; };
MeshSize{ PointsOf{ Volume{:}; } } = hs;
MeshSize{ PointsOf{ Volume{coil()}; } } = hc;
MeshSize{ PointsOf{ Volume{plate()}; } } = hp;
MeshSize{ PointsOf{ Volume{band()}; } } = hb;
