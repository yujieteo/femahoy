// The pulsating sphere at 500 Hz: a sphere of radius a in air, with the normal velocity U on
// all its surface. The sound pressure p solves the Helmholtz equation in the air:
//   div grad p + k^2 p = 0, k = omega / c.
// The octant x, y, z >= 0. On the 3 symmetry planes, dn p = 0 (the natural condition, a rigid
// wall). On the sphere, dn p = j omega rho U, with n into the sphere. On the outer sphere, dn p =
// -(j k + 1 / R) p: this condition is exact for the outgoing spherical wave.
// Second-order node functions on first-order tetrahedra. SI units. See ../derivation.md.
Group {
  Air = Region[1]; Source = Region[2]; Outer = Region[3];
  Domain = Region[{Air, Source, Outer}];
}
f = 500; c = 343; rho = 1.2; U = 1e-3; R = 0.5;
omega = 2 * Pi * f; k = omega / c;
FunctionSpace {
  { Name H; Type Form0;
    BasisFunction {
      { Name sn; NameOfCoef pn; Function BF_Node; Support Domain; Entity NodesOf[All]; }
      { Name se; NameOfCoef pe; Function BF_Node_2E; Support Domain; Entity EdgesOf[All]; }
    }
  }
}
Jacobian {
  { Name J; Case { { Region Region[{Source, Outer}]; Jacobian Sur; } { Region All; Jacobian Vol; } } }
}
Integration {
  { Name I; Case { { Type Gauss; Case {
    { GeoElement Tetrahedron; NumberOfPoints 15; }
    { GeoElement Triangle; NumberOfPoints 7; } } } } }
}
Formulation {
  { Name Helmholtz; Type FemEquation;
    Quantity { { Name p; Type Local; NameOfSpace H; } }
    Equation {
      Galerkin { [ Dof{d p}, {d p} ]; In Air; Jacobian J; Integration I; }
      Galerkin { [ -k^2 * Dof{p}, {p} ]; In Air; Jacobian J; Integration I; }
      Galerkin { [ Complex[1 / R, k] * Dof{p}, {p} ]; In Outer; Jacobian J; Integration I; }
      Galerkin { [ Complex[0, -omega * rho * U], {p} ]; In Source; Jacobian J; Integration I; }
    }
  }
}
Resolution {
  { Name Helmholtz;
    System { { Name S; NameOfFormulation Helmholtz; Type Complex; Frequency f; } }
    Operation { Generate[S]; Solve[S]; }
  }
}
// z = the mean of p on the sphere / (rho c U), the specific radiation impedance.
PostProcessing {
  { Name Helmholtz; NameOfFormulation Helmholtz;
    Quantity {
      { Name p; Value { Local { [ {p} ]; In Air; Jacobian J; } } }
      { Name p_abs; Value { Local { [ Norm[{p}] ]; In Air; Jacobian J; } } }
      { Name p_re; Value { Local { [ Re[{p}] ]; In Air; Jacobian J; } } }
      { Name p_im; Value { Local { [ Im[{p}] ]; In Air; Jacobian J; } } }
      { Name area; Value { Integral { [ 1 ]; In Source; Jacobian J; Integration I; } } }
      { Name int_re; Value { Integral { [ Re[{p}] ]; In Source; Jacobian J; Integration I; } } }
      { Name int_im; Value { Integral { [ Im[{p}] ]; In Source; Jacobian J; Integration I; } } }
    }
  }
}
// The pressure for the viewer, and in 1 row of z.txt for fh validate: z, and p in Pa at the
// point (0.2, 0.2, 0.1), r = 0.3 m.
PostOperation {
  { Name Helmholtz; NameOfPostProcessing Helmholtz;
    Operation {
      Print[ p, OnElementsOf Air, File "p.pos" ];
      Print[ p_abs, OnElementsOf Air, File "p_abs.pos" ];
      Print[ area[Source], OnGlobal, Format Table, StoreInVariable $area ];
      Print[ int_re[Source], OnGlobal, Format Table, StoreInVariable $int_re ];
      Print[ int_im[Source], OnGlobal, Format Table, StoreInVariable $int_im ];
      Print[ p_re, OnPoint {0.2, 0.2, 0.1}, Format Table, StoreInVariable $p_re ];
      Print[ p_im, OnPoint {0.2, 0.2, 0.1}, Format Table, StoreInVariable $p_im ];
      Echo[ "# z_re z_im p_re p_im", Format Table, File "z.txt" ];
      Print[ { $int_re / ($area * rho * c * U), $int_im / ($area * rho * c * U), $p_re, $p_im },
        Format "%.6e %.6e %.6e %.6e", File > "z.txt" ];
    }
  }
}
