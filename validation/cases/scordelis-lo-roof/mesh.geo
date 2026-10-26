// Full cylinder patch. The middle nodes stay on the cylinder.
If (!Exists(N))
  N=24;
EndIf
R=25000; A=40*Pi/180;
Point(1)={0,0,0};
Point(2)={0,-R*Sin(A),R*Cos(A)};
Point(3)={0,0,R}; Point(4)={0,R*Sin(A),R*Cos(A)};
Circle(1)={2,1,3}; Circle(2)={3,1,4};
Transfinite Curve{1,2}=N/2+1;
Extrude {50000,0,0} { Curve{1,2}; Layers{N}; Recombine; }
Mesh.ElementOrder=2; Mesh.SecondOrderIncomplete=1;
Mesh.SaveAll=1;
