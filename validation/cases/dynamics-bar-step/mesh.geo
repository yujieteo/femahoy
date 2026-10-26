L=200; W=10; NX=80; NY=4;
// Mapped box: transfinite edges, extruded layers, incomplete second order.
Point(1) = {0, 0, 0}; Point(2) = {L, 0, 0};
Point(3) = {L, W, 0}; Point(4) = {0, W, 0};
Line(1) = {1,2}; Line(2) = {2,3}; Line(3) = {3,4}; Line(4) = {4,1};
Curve Loop(1) = {1,2,3,4}; Plane Surface(1) = {1};
Transfinite Curve{1,3} = NX + 1; Transfinite Curve{2,4} = NY + 1;
Transfinite Surface{1}; Recombine Surface{1};
Extrude {0,0,10} { Surface{1}; Layers{4}; Recombine; }
Mesh.ElementOrder=1;
