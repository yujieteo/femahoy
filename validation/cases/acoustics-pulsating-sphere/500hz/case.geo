// The pulsating sphere: the air between the sphere r = a and the outer sphere r = R, in the
// octant x, y, z >= 0. Lengths in m.
SetFactory("OpenCASCADE");
// Mesh sizes at the sphere and at the outer sphere.
DefineConstant[ a = 0.1, R = 0.5, ha = 0.01, hR = 0.05 ];
Sphere(1) = {0, 0, 0, R, 0, Pi / 2, Pi / 2};
Sphere(2) = {0, 0, 0, a, 0, Pi / 2, Pi / 2};
BooleanDifference(3) = { Volume{1}; Delete; }{ Volume{2}; Delete; };
e = 1e-4;
source() = Surface In BoundingBox{-e, -e, -e, a + e, a + e, a + e};
outer() = Boundary{ Volume{3}; };
outer() -= source();
outer() -= Surface In BoundingBox{-e, -e, -e, e, R + e, R + e};
outer() -= Surface In BoundingBox{-e, -e, -e, R + e, e, R + e};
outer() -= Surface In BoundingBox{-e, -e, -e, R + e, R + e, e};
Physical Volume("air", 1) = {3};
Physical Surface("source", 2) = source();
Physical Surface("outer", 3) = outer();
MeshSize{ PointsOf{ Volume{3}; } } = hR;
MeshSize{ PointsOf{ Surface{source()}; } } = ha;
