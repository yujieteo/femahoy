// 2D eddy currents in 2 copper bars: 1 A in Bar1 and -1 A in Bar2 at the frequency Freq. The
// vector potential a_z on nodes (BF_PerpendicularEdge) and the voltage per length of each
// massive conductor (BF_RegionZ with the global quantities U and I). The box has the natural
// boundary condition, and a = 0 at 1 point fixes the constant of a. Lengths in m.
DefineConstant[ Freq = 1e5 ];
Group {
  Bar1 = Region[1]; Bar2 = Region[2]; Air = Region[3]; Gauge = Region[4];
  Bars = Region[{Bar1, Bar2}];
  Dom = Region[{Bar1, Bar2, Air}];
}
Function {
  mu0 = 4e-7 * Pi;
  nu[] = 1 / mu0;
  sigma[] = 5.8e7;
}
Constraint {
  { Name Fix; Case { { Region Gauge; Value 0; } } }
  { Name Current; Case { { Region Bar1; Value 1; } { Region Bar2; Value -1; } } }
}
Jacobian { { Name Vol; Case { { Region Dom; Jacobian Vol; } } } }
Integration {
  { Name Int; Case { { Type Gauss; Case { { GeoElement Triangle; NumberOfPoints 4; } } } } }
}
FunctionSpace {
  { Name Ha; Type Form1P;
    BasisFunction {
      { Name se; NameOfCoef ae; Function BF_PerpendicularEdge; Support Dom; Entity NodesOf[Dom]; }
    }
    Constraint { { NameOfCoef ae; EntityType NodesOf; NameOfConstraint Fix; } }
  }
  { Name Hu; Type Form1P;
    BasisFunction {
      { Name sr; NameOfCoef ur; Function BF_RegionZ; Support Bars; Entity Bars; }
    }
    GlobalQuantity {
      { Name U; Type AliasOf; NameOfCoef ur; }
      { Name I; Type AssociatedWith; NameOfCoef ur; }
    }
    Constraint { { NameOfCoef I; EntityType Region; NameOfConstraint Current; } }
  }
}
Formulation {
  { Name Eddy; Type FemEquation;
    Quantity {
      { Name a; Type Local; NameOfSpace Ha; }
      { Name ur; Type Local; NameOfSpace Hu; }
      { Name U; Type Global; NameOfSpace Hu [U]; }
      { Name I; Type Global; NameOfSpace Hu [I]; }
    }
    Equation {
      Galerkin { [ nu[] * Dof{d a}, {d a} ]; In Dom; Jacobian Vol; Integration Int; }
      Galerkin { DtDof [ sigma[] * Dof{a}, {a} ]; In Bars; Jacobian Vol; Integration Int; }
      Galerkin { [ sigma[] * Dof{ur}, {a} ]; In Bars; Jacobian Vol; Integration Int; }
      Galerkin { DtDof [ sigma[] * Dof{a}, {ur} ]; In Bars; Jacobian Vol; Integration Int; }
      Galerkin { [ sigma[] * Dof{ur}, {ur} ]; In Bars; Jacobian Vol; Integration Int; }
      GlobalTerm { [ Dof{I}, {U} ]; In Bars; }
    }
  }
}
Resolution {
  { Name Eddy;
    System { { Name A; NameOfFormulation Eddy; Type ComplexValue; Frequency Freq; } }
    Operation { Generate[A]; Solve[A]; SaveSolution[A]; }
  }
}
PostProcessing {
  { Name Eddy; NameOfFormulation Eddy;
    Quantity {
      { Name U; Value { Term { [ {U} ]; In Bars; } } }
      { Name I; Value { Term { [ {I} ]; In Bars; } } }
    }
  }
}
PostOperation {
  { Name Eddy; NameOfPostProcessing Eddy;
    Operation {
      Print[ U, OnRegion Bar1, Format Table, File "u1.txt" ];
      Print[ U, OnRegion Bar2, Format Table, File "u2.txt" ];
    }
  }
}
