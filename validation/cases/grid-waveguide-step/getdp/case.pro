// The reference of grid-waveguide-step (../derivation.md): TE10 through a symmetric H-plane step,
// with the time factor exp(j omega t), electric field e:
//   curl curl e - k0^2 e = 0, with e x n = 0 on the walls,
// and on each port the first-order condition of the TE10 wave:
//   n x curl e + j beta (n x e) x n = 2 j beta e_inc on port 1, 0 on port 2.
// Edge elements. SI units.
Group {
  Air = Region[1]; Port1 = Region[2]; Port2 = Region[3]; Pec = Region[4];
}
Function {
  f = 10e9; c0 = 299792458; k0 = 2 * Pi * f / c0;
  a1 = 0.02286; a2 = 0.016; x2 = 0.00343; b = 0.01016;
  beta1 = Sqrt[k0^2 - (Pi / a1)^2];
  beta2 = Sqrt[k0^2 - (Pi / a2)^2];
  e1[] = Vector[0, Sin[Pi * X[] / a1], 0];
  e2[] = Vector[0, Sin[Pi * (X[] - x2) / a2], 0];
  jb1[] = Complex[0, beta1];
  jb2[] = Complex[0, beta2];
}
Constraint {
  { Name E; Case { { Region Pec; Value 0; } } }
}
FunctionSpace {
  { Name Hcurl; Type Form1;
    BasisFunction {
      { Name se; NameOfCoef ee; Function BF_Edge; Support Region[{Air, Port1, Port2}]; Entity EdgesOf[All]; }
    }
    Constraint { { NameOfCoef ee; EntityType EdgesOf; NameOfConstraint E; } }
  }
}
Jacobian {
  { Name Vol; Case { { Region Air; Jacobian Vol; } } }
  { Name Sur; Case { { Region All; Jacobian Sur; } } }
}
Integration {
  { Name I; Case { { Type Gauss; Case {
    { GeoElement Tetrahedron; NumberOfPoints 4; }
    { GeoElement Triangle; NumberOfPoints 7; } } } } }
}
Formulation {
  { Name Wave; Type FemEquation;
    Quantity { { Name e; Type Local; NameOfSpace Hcurl; } }
    Equation {
      Galerkin { [ Dof{d e}, {d e} ]; In Air; Jacobian Vol; Integration I; }
      Galerkin { [ -k0^2 * Dof{e}, {e} ]; In Air; Jacobian Vol; Integration I; }
      Galerkin { [ jb1[] * Dof{e}, {e} ]; In Port1; Jacobian Sur; Integration I; }
      Galerkin { [ jb2[] * Dof{e}, {e} ]; In Port2; Jacobian Sur; Integration I; }
      Galerkin { [ -2 * jb1[] * e1[], {e} ]; In Port1; Jacobian Sur; Integration I; }
    }
  }
}
Resolution {
  { Name Wave;
    System { { Name S; NameOfFormulation Wave; Type Complex; Frequency f; } }
    Operation { Generate[S]; Solve[S]; }
  }
}
// The projection of e on each mode: S11 = (e, e1) / (e1, e1) - 1 and
// S21 = (e, e2) / (e2, e2) * sqrt(a2 beta2 / (a1 beta1)), the waves normalized to 1 W.
PostProcessing {
  { Name Wave; NameOfFormulation Wave;
    Quantity {
      { Name p1; Value { Integral { [ {e} * e1[] / (a1 * b / 2) ]; In Port1; Jacobian Sur; Integration I; } } }
      { Name p2; Value { Integral { [ {e} * e2[] / (a2 * b / 2) * Sqrt[a2 * beta2 / (a1 * beta1)] ]; In Port2; Jacobian Sur; Integration I; } } }
    }
  }
}
PostOperation {
  { Name Wave; NameOfPostProcessing Wave;
    Operation {
      Print[ p1[Port1], OnGlobal, Format Table, File "p1.txt" ];
      Print[ p2[Port2], OnGlobal, Format Table, File "p2.txt" ];
    }
  }
}
