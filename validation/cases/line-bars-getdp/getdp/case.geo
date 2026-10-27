// 2 copper bars side by side in a square box: the reference model of GetDP
// (../derivation.md). Lengths in m. H is the mesh size at the bars, B in the box.
DefineConstant[ H = 25e-6, B = 2e-3 ];
L = 20e-3;
Point(1) = {-L, -L, 0, B}; Point(2) = {L, -L, 0, B};
Point(3) = {L, L, 0, B}; Point(4) = {-L, L, 0, B};
Line(1) = {1, 2}; Line(2) = {2, 3}; Line(3) = {3, 4}; Line(4) = {4, 1};
Curve Loop(1) = {1, 2, 3, 4};
// Bar 1: x -2.5 to -0.5 mm, bar 2: x 0.5 to 2.5 mm, both y -0.5 to 0.5 mm.
Point(11) = {-2.5e-3, -0.5e-3, 0, H}; Point(12) = {-0.5e-3, -0.5e-3, 0, H};
Point(13) = {-0.5e-3, 0.5e-3, 0, H}; Point(14) = {-2.5e-3, 0.5e-3, 0, H};
Line(11) = {11, 12}; Line(12) = {12, 13}; Line(13) = {13, 14}; Line(14) = {14, 11};
Curve Loop(2) = {11, 12, 13, 14};
Point(21) = {0.5e-3, -0.5e-3, 0, H}; Point(22) = {2.5e-3, -0.5e-3, 0, H};
Point(23) = {2.5e-3, 0.5e-3, 0, H}; Point(24) = {0.5e-3, 0.5e-3, 0, H};
Line(21) = {21, 22}; Line(22) = {22, 23}; Line(23) = {23, 24}; Line(24) = {24, 21};
Curve Loop(3) = {21, 22, 23, 24};
Plane Surface(1) = {1, 2, 3};
Plane Surface(2) = {2};
Plane Surface(3) = {3};
// The size grows away from the bars, as in the mesh of the built-in solver.
Field[1] = Distance; Field[1].CurvesList = {11, 12, 13, 14, 21, 22, 23, 24};
Field[1].Sampling = 400;
Field[2] = MathEval; Field[2].F = Sprintf("%g + 0.1 * F1", H);
Field[3] = Min; Field[3].FieldsList = {2};
Background Field = 3;
Mesh.MeshSizeExtendFromBoundary = 0;
Mesh.MeshSizeFromPoints = 0;
Mesh.MeshSizeMax = B;
Physical Surface("Bar1", 1) = {2};
Physical Surface("Bar2", 2) = {3};
Physical Surface("Air", 3) = {1};
Physical Point("Gauge", 4) = {1};
