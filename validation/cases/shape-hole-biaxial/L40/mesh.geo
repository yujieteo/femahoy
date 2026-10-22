// A quarter of a square plate with a half side of 40 and a hole at the centre. The hole is the
// superellipse |x/a|^power + |y/b|^power = 1, with a/b = ratio and a*b = 1. The loop of fh run
// sets ratio and power with -setnumber.
SetFactory("OpenCASCADE");
DefineConstant[ ratio = 1, power = 2 ];
L = 40;
// The element size on the hole edge. The size grows with the distance from the hole.
h = 0.025;
a = Sqrt(ratio);
b = 1 / Sqrt(ratio);
// The hole edge is a spline through 61 points from (a, 0) to (0, b).
N = 61;
For i In {0:N - 1}
  t = Pi / 2 * i / (N - 1);
  Point(100 + i) = {a * Cos(t)^(2 / power), b * Sin(t)^(2 / power), 0};
EndFor
Spline(1) = {100:100 + N - 1};
Point(1) = {L, 0, 0};
Point(2) = {L, L, 0};
Point(3) = {0, L, 0};
Line(2) = {100, 1};
Line(3) = {1, 2};
Line(4) = {2, 3};
Line(5) = {3, 100 + N - 1};
Curve Loop(1) = {2, 3, 4, 5, -1};
Plane Surface(1) = {1};
Physical Surface("PLATE") = {1};
Physical Curve("HOLE") = {1};
Physical Curve("YSYM") = {2};
Physical Curve("XSYM") = {5};
Physical Curve("RIGHT") = {3};
Physical Curve("TOP") = {4};
Field[1] = Distance;
Field[1].CurvesList = {1};
Field[1].Sampling = 400;
Field[2] = MathEval;
Field[2].F = Sprintf("%g + 0.15 * F1", h);
Background Field = 2;
Mesh.MeshSizeExtendFromBoundary = 0;
Mesh.MeshSizeFromPoints = 0;
Mesh.MeshSizeFromCurvature = 0;
Mesh.MeshSizeMax = L / 10;
// Six-node triangles (CPS6). ccx reads the node sets and the element set PLATE.
Mesh.ElementOrder = 2;
Mesh.SaveGroupsOfNodes = 1;
Mesh.SaveGroupsOfElements = -100;
