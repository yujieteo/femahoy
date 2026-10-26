L=25; W=5; NX=13; NY=3;
// Mapped box: transfinite edges, extruded layers, incomplete second order.
Point(1) = {0, 0, 0}; Point(2) = {L, 0, 0};
Point(3) = {L, W, 0}; Point(4) = {0, W, 0};
Line(1) = {1,2}; Line(2) = {2,3}; Line(3) = {3,4}; Line(4) = {4,1};
Curve Loop(1) = {1,2,3,4}; Plane Surface(1) = {1};
Transfinite Curve{1,3} = NX + 1; Transfinite Curve{2,4} = NY + 1;
Transfinite Surface{1}; Recombine Surface{1};
si() = Extrude {0,0,0.73} { Surface{1}; Layers{2}; Recombine; };
Extrude {0,0,0.02} { Surface{si(0)}; Layers{1}; Recombine; }
Mesh.ElementOrder=2; Mesh.SecondOrderIncomplete=1;
