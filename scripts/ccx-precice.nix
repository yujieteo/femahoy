# ccx_preCICE: CalculiX 2.20 with the preCICE adapter 2.20.2, for a preCICE case of fh run (README.md,
# "Install"), with Nix. Both are GPL. fh only starts the program, and links no code of it.
# Build: nix build -f scripts/ccx-precice.nix -o ccx-precice
{
  pkgs ? import <nixpkgs> { },
}:
pkgs.stdenv.mkDerivation {
  pname = "ccx-precice";
  version = "2.20.2";
  srcs = [
    (pkgs.fetchurl {
      url = "https://www.dhondt.de/ccx_2.20.src.tar.bz2";
      hash = "sha256-Y79uoJ5+3K6T4BRbG7BXnqeuguBG9gdaJ8gUW3J2G88=";
    })
    (pkgs.fetchFromGitHub {
      owner = "precice";
      repo = "calculix-adapter";
      rev = "v2.20.2";
      hash = "sha256-x7QrHLJnJFkUShvSiSO3URDx0Z14MMQYfRN+8u6V7Vs=";
    })
  ];
  sourceRoot = "source";
  nativeBuildInputs = [
    pkgs.gfortran
    pkgs.pkg-config
  ];
  buildInputs = [
    pkgs.arpack
    pkgs.spooles
    pkgs.blas
    pkgs.lapack
    pkgs.precice
  ];
  env.NIX_CFLAGS_COMPILE = "-std=gnu17 -Wno-return-mismatch";
  makeFlags = [
    "CCX=../CalculiX/ccx_2.20/src"
    "SPOOLES_INCLUDE=-I${pkgs.spooles}/include/spooles"
    # gcc compiles all the sources. Thus the C++ library is libstdc++, also on macOS.
    "CC=gcc"
    "CXX=g++"
    "FC=gfortran"
    "ADDITIONAL_FFLAGS=-std=legacy"
  ];
  enableParallelBuilding = true;
  preBuild = ''
    mkdir -p bin
    makeFlagsArray+=("LIBS=-lspooles -lprecice -larpack -llapack -lblas -lpthread -lm -lstdc++")
  '';
  installPhase = "install -Dm0755 bin/ccx_preCICE $out/bin/ccx_preCICE";
}
