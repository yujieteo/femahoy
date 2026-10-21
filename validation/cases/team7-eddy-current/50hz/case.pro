// TEAM problem 7 at 50 Hz: time-harmonic eddy currents in the plate, with the modified vector
// potential a (E = -j omega a in the plate):
//   curl(nu curl a) + j omega sigma a = js.
// Edge elements. A tree gauge fixes a in the coil and the air. The shell maps to infinity.
// SI units. See ../derivation.md.
Group {
  Plate = Region[1]; Coil = Region[2]; Air = Region[{3, 6}]; Band = Region[6];
  Shell = Region[7]; Outer = Region[4]; PlateSkin = Region[5];
  Domain = Region[{Plate, Coil, Air, Shell}];
  NonConducting = Region[{Coil, Air, Shell}];
}
Function {
  nu[] = 1 / (4e-7 * Pi);
  sigma[] = 3.526e7;
  // 2742 ampere-turns on the coil section of 0.025 x 0.100. The current goes anticlockwise as
  // seen from +z, around the square (0.144..0.244, 0.050..0.150) on the coil axis.
  px[] = Min[Max[X[], 0.144], 0.244];
  py[] = Min[Max[Y[], 0.050], 0.150];
  js[] = 2742 / (0.025 * 0.100) * Vector[py[] - Y[], X[] - px[], 0]
    / Norm[Vector[X[] - px[], Y[] - py[], 0]];
}
Constraint {
  { Name A; Case { { Region Outer; Value 0; } } }
  { Name Gauge; Case { { Region NonConducting; SubRegion Region[{Outer, PlateSkin}]; Value 0; } } }
}
FunctionSpace {
  // The tree of the gauge starts with a tree on the skin of the plate, and fixes its edges too.
  // The edges of the skin thus have their own coefficient, which the gauge does not fix.
  { Name Hcurl; Type Form1;
    BasisFunction {
      { Name se; NameOfCoef ae; Function BF_Edge; Support Domain; Entity EdgesOf[All, Not PlateSkin]; }
      { Name ss; NameOfCoef as; Function BF_Edge; Support Domain; Entity EdgesOf[PlateSkin]; }
    }
    Constraint {
      { NameOfCoef ae; EntityType EdgesOf; NameOfConstraint A; }
      { NameOfCoef ae; EntityType EdgesOfTreeIn; EntitySubType StartingOn; NameOfConstraint Gauge; }
    }
  }
  // B as a continuous field in the band: the L2 projection of curl a on nodal functions.
  { Name Hgrad; Type Vector;
    BasisFunction {
      { Name sx; NameOfCoef bx; Function BF_NodeX; Support Band; Entity NodesOf[All]; }
      { Name sy; NameOfCoef by; Function BF_NodeY; Support Band; Entity NodesOf[All]; }
      { Name sz; NameOfCoef bz; Function BF_NodeZ; Support Band; Entity NodesOf[All]; }
    }
  }
}
Jacobian {
  { Name Vol; Case {
    { Region Shell; Jacobian VolSphShell{0.35, 0.45, 0.147, 0.147, 0.074}; }
    { Region All; Jacobian Vol; } } }
}
Integration { { Name I; Case { { Type Gauss; Case { { GeoElement Tetrahedron; NumberOfPoints 4; } } } } } }
Formulation {
  { Name Eddy; Type FemEquation;
    Quantity { { Name a; Type Local; NameOfSpace Hcurl; } }
    Equation {
      Galerkin { [ nu[] * Dof{d a}, {d a} ]; In Domain; Jacobian Vol; Integration I; }
      Galerkin { DtDof [ sigma[] * Dof{a}, {a} ]; In Plate; Jacobian Vol; Integration I; }
      Galerkin { [ -js[], {a} ]; In Coil; Jacobian Vol; Integration I; }
    }
  }
  { Name Smooth; Type FemEquation;
    Quantity {
      { Name a; Type Local; NameOfSpace Hcurl; }
      { Name b; Type Local; NameOfSpace Hgrad; }
    }
    Equation {
      Galerkin { [ Dof{b}, {b} ]; In Band; Jacobian Vol; Integration I; }
      Galerkin { [ -{d a}, {b} ]; In Band; Jacobian Vol; Integration I; }
    }
  }
}
Resolution {
  { Name Eddy;
    System {
      { Name S; NameOfFormulation Eddy; Type Complex; Frequency 50; }
      { Name P; NameOfFormulation Smooth; Type Complex; Frequency 50; }
    }
    Operation { Generate[S]; Solve[S]; Generate[P]; Solve[P]; }
  }
}
// With the time factor exp(j omega t), the measured Bz at omega t = 0 and 90 degrees is Re(Bz)
// and -Im(Bz).
PostProcessing {
  { Name Eddy; NameOfFormulation Smooth;
    Quantity {
      { Name j; Value { Local { [ -sigma[] * Dt[{a}] ]; In Plate; Jacobian Vol; } } }
      { Name bz0; Value { Local { [ Re[CompZ[{b}]] ]; In Band; Jacobian Vol; } } }
      { Name bz90; Value { Local { [ -Im[CompZ[{b}]] ]; In Band; Jacobian Vol; } } }
    }
  }
}
// The eddy current density for the viewer, and Bz in T at 3 points of the line A1-B1
// (y = 0.072, z = 0.034) in 1 row of bz.txt for fh validate.
PostOperation {
  { Name Eddy; NameOfPostProcessing Eddy;
    Operation {
      Print[ j, OnElementsOf Plate, File "j.pos" ];
      Print[ bz0, OnPoint {0.036, 0.072, 0.034}, Format Table, StoreInVariable $bz0_36 ];
      Print[ bz90, OnPoint {0.036, 0.072, 0.034}, Format Table, StoreInVariable $bz90_36 ];
      Print[ bz0, OnPoint {0.126, 0.072, 0.034}, Format Table, StoreInVariable $bz0_126 ];
      Print[ bz90, OnPoint {0.126, 0.072, 0.034}, Format Table, StoreInVariable $bz90_126 ];
      Print[ bz0, OnPoint {0.162, 0.072, 0.034}, Format Table, StoreInVariable $bz0_162 ];
      Print[ bz90, OnPoint {0.162, 0.072, 0.034}, Format Table, StoreInVariable $bz90_162 ];
      Echo[ "# bz0_36 bz90_36 bz0_126 bz90_126 bz0_162 bz90_162", Format Table, File "bz.txt" ];
      Print[ { $bz0_36, $bz90_36, $bz0_126, $bz90_126, $bz0_162, $bz90_162 },
        Format "%.6e %.6e %.6e %.6e %.6e %.6e", File > "bz.txt" ];
    }
  }
}
