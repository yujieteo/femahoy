#!/usr/bin/env bash
# Install femahoy from this repository for this user, with no root: build `fh` and the wasm
# viewer, and install gmsh and each solver of DESIGN.md section 6.
#
#   scripts/install.sh [--prefix DIR] [--venv DIR]
#
# The script runs on macOS (Apple Silicon) and on Linux (x86-64 and ARM64). It needs bash, git,
# tar, curl or wget, and on macOS the Xcode command line tools (xcode-select --install). It
# installs no system package.
#
# - micromamba installs Rust, gmsh and most solvers from conda-forge. The script downloads
#   GetDP from getdp.info and, on macOS, OpenFOAM.app from github.com/gerlero/openfoam-app. It
#   builds preCICE with its 2 adapters, LAMMPS and openEMS from source. It checks each download
#   with its SHA-256. All of it goes to the data folder ${XDG_DATA_HOME:-~/.local/share}/femahoy.
# - The prefix (default ~/.local/bin) gets fh, the folder viewer beside fh, where `fh serve`
#   looks for it, fh-python and solvers.env. solvers.env has 1 line FH_NAME=PATH for each solver
#   that the script installed. fh reads it at start, and a variable of the environment has
#   priority (README.md, "Environment variables").
# - When the variable FH_NAME of a solver is set, the script does not install that solver.
# - fh-python runs the Python of the venv (default DATA/venv), which has Python 3.12 and
#   build123d.
# - A second run builds fh again and updates the files and the venv. It keeps each solver that
#   has the same version.
#
# At the end, the script names each solver that it did not install, with the analysis types that
# it disables. The exit code is 1 when the build of fh or the install of a solver failed. A solver
# with no build for this system is not a failure.
set -euo pipefail

usage="usage: scripts/install.sh [--prefix DIR] [--venv DIR]"
data=${XDG_DATA_HOME:-$HOME/.local/share}/femahoy
prefix=$HOME/.local/bin
venv=$data/venv
while [ $# -gt 0 ]; do
  case $1 in
    --prefix | --venv)
      if [ $# -lt 2 ] || [ -z "$2" ]; then
        printf 'install: %s needs a folder\n%s\n' "$1" "$usage" >&2
        exit 2
      fi
      if [ "$1" = --prefix ]; then prefix=$2; else venv=$2; fi
      shift 2
      ;;
    --prefix=?*) prefix=${1#--prefix=}; shift ;;
    --venv=?*) venv=${1#--venv=}; shift ;;
    -h | --help) echo "$usage"; exit 0 ;;
    *) printf 'install: unknown argument: %s\n%s\n' "$1" "$usage" >&2; exit 2 ;;
  esac
done
case $prefix in /*) ;; *) prefix=$PWD/$prefix ;; esac
case $venv in /*) ;; *) venv=$PWD/$venv ;; esac

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
target=${CARGO_TARGET_DIR:-target}
case $target in /*) ;; *) target=$root/$target ;; esac
toolchain=$(awk -F'"' '/^channel *=/ { print $2 }' rust-toolchain.toml)
bindgen=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print }' Cargo.lock)

case $(uname -s)-$(uname -m) in
  Darwin-arm64) platform=osx-arm64 ;;
  Linux-x86_64) platform=linux-64 ;;
  Linux-aarch64) platform=linux-aarch64 ;;
  *)
    echo "install: femahoy installs on macOS (Apple Silicon) and Linux (x86-64, ARM64), not on $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

# Check the tools of the system.
missing=()
for tool in git tar; do
  command -v $tool >/dev/null 2>&1 || missing+=("$tool: install it from your package manager")
done
if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
  missing+=("curl or wget: install 1 of them from your package manager")
fi
if [ $platform = osx-arm64 ] && ! xcode-select -p >/dev/null 2>&1; then
  missing+=("the Xcode command line tools (the C compiler and linker): xcode-select --install")
fi
if [ ${#missing[@]} -gt 0 ]; then
  echo "install: ${#missing[@]} tool(s) are missing. Install them, then run scripts/install.sh again:" >&2
  for line in "${missing[@]}"; do
    echo "  - $line" >&2
  done
  exit 1
fi

envs=$data/conda/envs
opt=$data/opt
logs=$data/logs
mm=$data/micromamba
mkdir -p "$envs" "$opt" "$logs"
export MAMBA_ROOT_PREFIX=$data/conda
jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
# 1 compile of preCICE needs up to 2 GB of memory, so 1 job for each 2 GB.
case $platform in
  osx-*) memory=$(($(sysctl -n hw.memsize) / 1073741824)) ;;
  *) memory=$(($(awk '/^MemTotal:/ { print $2 }' /proc/meminfo) / 1048576)) ;;
esac
if [ $((memory / 2)) -lt "$jobs" ]; then jobs=$((memory / 2)); fi
if [ "$jobs" -lt 1 ]; then jobs=1; fi

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | awk '{ print $1 }'
}

# Download the URL to FILE and check its SHA-256.
fetch() {
  local url=$1 file=$2 sum=$3 have
  if [ -f "$file" ] && [ "$(sha256 "$file")" = "$sum" ]; then return; fi
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 -o "$file.part" "$url"
  else
    wget -q -O "$file.part" "$url"
  fi
  have=$(sha256 "$file.part")
  if [ "$have" != "$sum" ]; then
    echo "install: $url has the SHA-256 $have, not $sum" >&2
    rm -f "$file.part"
    return 1
  fi
  mv "$file.part" "$file"
}

# Make the conda-forge environment $envs/NAME with the packages SPEC..., unless it has them.
conda_env() {
  local env=$envs/$1 spec="${*:2}"
  if [ "$(cat "$env/.femahoy" 2>/dev/null)" = "$spec" ]; then return; fi
  rm -rf "$env"
  "$mm" create -y -q -p "$env" -c conda-forge --override-channels "${@:2}" || return 1
  echo "$spec" >"$env/.femahoy"
}

# True when the folder DIR was built from KEY. done_with DIR KEY records it, after the check of
# the build.
built_from() { [ "$(cat "$1/.femahoy" 2>/dev/null)" = "$2" ]; }
done_with() { echo "$2" >"$1/.femahoy"; }

echo "install: micromamba"
case $platform in
  osx-arm64) sum=ec2a072f028e1a7cf20f3e2e74d5a8127cf5a5f27636375b5359811565f4e5be ;;
  linux-64) sum=366cd9cd8be14df1ab8ed50352a82111082a36686b2d389fdb79a92c3fafb3e3 ;;
  linux-aarch64) sum=9f93b974adcb4d166996af969b6cd371287d1a3e52733704727884d9b74cb7a7 ;;
esac
fetch "https://github.com/mamba-org/micromamba-releases/releases/download/2.9.0-0/micromamba-$platform" "$mm" $sum
chmod 755 "$mm"

echo "install: Rust $toolchain and wasm-bindgen $bindgen from conda-forge"
build=(rust="$toolchain" rust-std-wasm32-unknown-unknown="$toolchain" wasm-bindgen-cli="$bindgen" uv)
# On Linux, the C compiler and linker of conda-forge link fh. On macOS, Xcode links it.
case $platform in linux-*) build+=(c-compiler) ;; esac
conda_env build "${build[@]}" >"$logs/build.log" 2>&1 || {
  tail -20 "$logs/build.log" >&2
  exit 1
}
# The conda-forge Rust is not rustup, so it does not read rust-toolchain.toml.
PATH=$envs/build/bin:$PATH
echo "install: build fh (release)"
cargo build --release --locked --bin fh
echo "install: build the wasm viewer"
scripts/build-viewer.sh "$target/release/viewer"
# The tested version of build123d.
echo "install: make the Python environment with build123d in $venv"
uv venv --quiet --allow-existing -p 3.12 "$venv"
uv pip install --quiet -p "$venv/bin/python" build123d==0.13.0

# Each solver: ID|VARIABLE|NAME|the analysis types that need it.
solvers=(
  "ccx|FH_CCX|CalculiX|structures, explicit dynamics, random vibration, topology and shape optimization"
  "gmsh|FH_GMSH|gmsh|fh mesh, electromagnetics, acoustics, shape optimization"
  "openfoam|FH_OPENFOAM|OpenFOAM|CFD, fluid-structure interaction"
  "getdp|FH_GETDP|GetDP|electromagnetics, acoustics"
  "precice|FH_CCX_PRECICE|preCICE with its CalculiX and OpenFOAM adapters|fluid-structure interaction"
  "pychrono|FH_PYCHRONO|PyChrono|multibody dynamics"
  "lammps|FH_LAMMPS|LAMMPS|particles (SPH, DEM)"
  "meep|FH_MEEP|Meep|optics and photonics"
  "openems|FH_OPENEMS|openEMS|antennas"
)

# Each install_ID function installs the solver ID, checks that it starts, and prints its line
# VARIABLE=PATH of solvers.env last. When it cannot install the solver on this system, it prints
# the cause last and no such line.

install_ccx() {
  conda_env ccx calculix=2.23
  # ccx -v, ccx_preCICE -v and lmp -h end with an exit code that is not 0.
  { "$envs/ccx/bin/ccx" -v || true; } | grep 2.23
  echo "FH_CCX=$envs/ccx/bin/ccx"
}

install_gmsh() {
  conda_env gmsh gmsh=4.15.2
  "$envs/gmsh/bin/gmsh" --version
  echo "FH_GMSH=$envs/gmsh/bin/gmsh"
}

# The program that runs each program of OpenFOAM as `openfoam PROGRAM ARGS`.
openfoam=
install_openfoam() {
  case $platform in
    osx-arm64)
      local app=$opt/OpenFOAM-v2512.app
      if ! built_from "$app" v2.2.3; then
        fetch https://github.com/gerlero/openfoam-app/releases/download/v2.2.3/openfoam2512-app-arm64.zip \
          "$opt/openfoam.zip" e1bf2c1b58a6d125199d34b78dbf1372984210381edbc62812be4b8d6c8d97f1
        rm -rf "$app"
        unzip -q "$opt/openfoam.zip" -d "$opt"
        rm "$opt/openfoam.zip"
        done_with "$app" v2.2.3
      fi
      openfoam=$app/Contents/Resources/etc/openfoam
      ;;
    linux-64)
      # The build tools and libraries of preCICE and its adapters, in the environment of
      # OpenFOAM: the adapter is a library that OpenFOAM loads, so both need the same C++ library.
      # binutils gives ar and ld by their plain names, which a Linux with no build tools has not.
      conda_env openfoam openfoam=2412 cmake make pkg-config bzip2 binutils c-compiler cxx-compiler \
        fortran-compiler libboost-devel eigen libxml2-devel arpack spooles libblas liblapack
      # wmake looks for the headers of OpenFOAM in $WM_PROJECT_DIR/src.
      ln -sfn include/OpenFOAM-2412/src "$envs/openfoam/src"
      openfoam=$envs/openfoam/bin/fh-openfoam
      # The environment of a conda-forge OpenFOAM comes from its activation scripts. Its wmake
      # names the compilers $GCC and $GXX, which the compilers of conda-forge do not set.
      cat >"$openfoam" <<EOF
#!/usr/bin/env bash
export CONDA_PREFIX='$envs/openfoam' GCC=gcc GXX=g++
export PATH="\$CONDA_PREFIX/bin:\$PATH"
for f in "\$CONDA_PREFIX"/etc/conda/activate.d/*.sh; do . "\$f"; done
exec "\$@"
EOF
      chmod 755 "$openfoam"
      ;;
    *) echo "OpenFOAM has no build for $platform"; return ;;
  esac
  "$openfoam" blockMesh -help >/dev/null
  echo "FH_OPENFOAM=$openfoam"
}

install_getdp() {
  local dir url sum
  case $platform in
    osx-arm64)
      dir=getdp-4.0.0-MacOSARM url=https://getdp.info/bin/macOS/getdp-4.0.0-MacOSARMc.tgz
      sum=6fe77e17aac2ec29aefd12e70db80fd54033016d55ed3e2c4c1e1470afd27b2f
      ;;
    linux-64)
      dir=getdp-3.5.0-Linux64 url=https://getdp.info/bin/Linux/getdp-3.5.0-Linux64c.tgz
      sum=d3c28fa18f20d6147b4c7367d4dd802e9f7ddb58c608688bbb71919dbca8041d
      ;;
    *) echo "GetDP has no build for $platform"; return ;;
  esac
  if [ ! -x "$opt/$dir/bin/getdp" ]; then
    fetch "$url" "$opt/getdp.tgz" $sum
    tar xzf "$opt/getdp.tgz" -C "$opt"
    rm "$opt/getdp.tgz"
  fi
  "$opt/$dir/bin/getdp" -version
  echo "FH_GETDP=$opt/$dir/bin/getdp"
}

# preCICE 3.4.1 with no MPI, CalculiX 2.20 with the preCICE adapter (ccx_preCICE) and the
# OpenFOAM adapter, built from source for the OpenFOAM of install_openfoam.
install_precice() {
  if [ -z "$openfoam" ]; then
    echo "the adapter needs the OpenFOAM of this script, and it did not install OpenFOAM"
    return
  fi
  local env src key=precice-3.4.1-calculix-adapter-2.20.2-openfoam-adapter-1.4.0
  case $platform in
    osx-arm64)
      conda_env precice cmake make pkg-config bzip2 c-compiler cxx-compiler fortran-compiler llvm-openmp \
        libboost-devel eigen libxml2-devel arpack spooles libblas liblapack
      env=$envs/precice
      ;;
    *) env=$envs/openfoam ;;
  esac
  if ! built_from "$env/bin" $key; then
    src=$data/src/precice
    rm -rf "$src"
    mkdir -p "$src"
    fetch https://github.com/precice/precice/archive/refs/tags/v3.4.1.tar.gz "$src/precice.tgz" \
      ef4713c938a1b2000d0b071175e1b45f9ec55c7aec4bbe7b65c3992edcc74ac7
    fetch https://www.dhondt.de/ccx_2.20.src.tar.bz2 "$src/ccx.tar.bz2" \
      63bf6ea09e7edcae93e0145b1bb0579ea7ae82e046f6075a27c8145b72761bcf
    fetch https://github.com/precice/calculix-adapter/archive/refs/tags/v2.20.2.tar.gz "$src/ccx-adapter.tgz" \
      033f6e61e707283df708d3eda3c5930146bb7f8acd3a0469ab707e77cba30199
    fetch https://github.com/precice/openfoam-adapter/releases/download/v1.4.0/openfoam-adapter-1.4.0-OpenFOAMv1812-v2606-newer.tar.gz \
      "$src/openfoam-adapter.tgz" 399c89ab0fc5dfcff87f77433dddb60d4de0b2071de9b78b0e756e4079fd6a21
    for archive in "$src"/*.tgz; do tar xzf "$archive" -C "$src"; done
    (
      # The compilers and the tools of the environment, also bzip2 for GNU tar.
      PATH=$env/bin:$PATH
      tar xjf "$src/ccx.tar.bz2" -C "$src"
      # On Fedora, CMake puts the libraries in lib64 by default.
      cmake -S "$src/precice-3.4.1" -B "$src/build" -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$env" -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_PREFIX_PATH="$env" \
        -DBUILD_SHARED_LIBS=ON -DBUILD_TESTING=OFF -DPRECICE_FEATURE_MPI_COMMUNICATION=OFF \
        -DPRECICE_FEATURE_PETSC_MAPPING=OFF -DPRECICE_FEATURE_PYTHON_ACTIONS=OFF \
        -DCMAKE_INSTALL_RPATH="$env/lib" -DCMAKE_INSTALL_NAME_DIR="$env/lib"
      cmake --build "$src/build" -j "$jobs"
      cmake --install "$src/build"
      local cc=gcc cxx=g++ cxxlib=-lstdc++
      if [ $platform = osx-arm64 ]; then cc=clang cxx=clang++ cxxlib=-lc++; fi
      cd "$src/calculix-adapter-2.20.2"
      mkdir -p bin
      make -j "$jobs" CCX="$src/CalculiX/ccx_2.20/src" SPOOLES_INCLUDE="-I$env/include/spooles" \
        CC=$cc CXX=$cxx FC=gfortran ADDITIONAL_FFLAGS=-std=legacy \
        LIBS="-L$env/lib -Wl,-rpath,$env/lib $env/lib/spoolesMT.a $env/lib/spooles.a -lprecice -larpack -llapack -lblas -lpthread -lm $cxxlib"
      install -m 755 bin/ccx_preCICE "$env/bin/ccx_preCICE"
    )
    # OpenFOAM compiles the adapter with its own compiler. It needs only the pkg-config of the
    # environment, which finds preCICE.
    mkdir -p "$src/pkg-config"
    printf '#!/bin/sh\nexec %s/bin/pkg-config "$@"\n' "$env" >"$src/pkg-config/pkg-config"
    chmod 755 "$src/pkg-config/pkg-config"
    # Allwmake puts the adapter in the user library folder of OpenFOAM ($FOAM_USER_LIBBIN). Its
    # last check runs ldd, which macOS does not have, so the check below replaces its exit code.
    # PATH is set in the OpenFOAM shell, because a zsh start file can set PATH again on macOS.
    (cd "$src/openfoam-adapter-1.4.0" &&
      "$openfoam" bash -c 'PATH=$1:$PATH PKG_CONFIG_PATH=$2 ./Allwmake' - "$src/pkg-config" "$env/lib/pkgconfig") || true
    # A glob makes the bash of the macOS OpenFOAM app crash.
    "$openfoam" bash -c 'ls "$FOAM_USER_LIBBIN"' | grep libpreciceAdapterFunctionObject
    rm -rf "$src"
  fi
  { "$env/bin/ccx_preCICE" -v || true; } | grep 2.20
  done_with "$env/bin" $key
  echo "FH_CCX_PRECICE=$env/bin/ccx_preCICE"
}

install_pychrono() {
  conda_env pychrono pychrono=10.0.0 python=3.12
  "$envs/pychrono/bin/python" -c "import pychrono"
  echo "FH_PYCHRONO=$envs/pychrono/bin/python"
}

# LAMMPS with its basic packages and SPH, built from source: the LAMMPS of conda-forge has no SPH
# package. On macOS, Xcode compiles it. Its CMake needs Python, which Xcode has.
install_lammps() {
  local dir=$opt/lammps env=$envs/lammps version=stable_22Jul2025_update6 src
  local spec=(cmake make)
  case $platform in linux-*) spec+=(binutils c-compiler cxx-compiler python) ;; esac
  conda_env lammps "${spec[@]}"
  if ! built_from "$dir" $version; then
    src=$data/src/lammps
    rm -rf "$src" "$dir"
    mkdir -p "$src"
    fetch https://github.com/lammps/lammps/archive/refs/tags/$version.tar.gz "$src/lammps.tgz" \
      40780d0269c656ffa983a8e3e6ed047fedfe1d0b7344b77ace25bf8c7c535f01
    tar xzf "$src/lammps.tgz" -C "$src"
    (
      PATH=$env/bin:$PATH
      cmake -S "$src/lammps-$version/cmake" -B "$src/build" \
        -C "$src/lammps-$version/cmake/presets/basic.cmake" -DPKG_SPH=ON -DBUILD_MPI=OFF \
        -DBUILD_OMP=OFF -DFFT=KISS -DWITH_JPEG=OFF -DWITH_PNG=OFF -DWITH_FFMPEG=OFF \
        -DDOWNLOAD_POTENTIALS=OFF -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$dir" \
        -DCMAKE_INSTALL_RPATH="$env/lib"
      cmake --build "$src/build" -j "$jobs"
      cmake --install "$src/build"
    )
    rm -rf "$src"
  fi
  { "$dir/bin/lmp" -h || true; } | grep -w SPH
  done_with "$dir" $version
  echo "FH_LAMMPS=$dir/bin/lmp"
}

install_meep() {
  conda_env meep "pymeep=1.35.0=nompi*" python=3.12
  "$envs/meep/bin/python" -c "import meep"
  echo "FH_MEEP=$envs/meep/bin/python"
}

# openEMS 0.37.0, built from source with its script update_openEMS.sh and the libraries of
# conda-forge. Its Python modules go to a venv that sees the packages of the environment.
install_openems() {
  local dir=$opt/openEMS env=$envs/openems commit=974294ba7d812ef5a46a4760593057aee7ce6866 src
  local spec=(cmake make pkg-config libboost-devel "cgal-cpp>=6" hdf5 vtk-base vtk-io-ffmpeg tinyxml
    python=3.12 cython numpy h5py setuptools pip)
  # On macOS, Xcode compiles openEMS: Python builds its modules with clang, which must be the
  # clang of Xcode. On Linux, VTK also needs the TBB headers.
  case $platform in linux-*) spec+=(c-compiler cxx-compiler tbb-devel) ;; esac
  conda_env openems "${spec[@]}"
  if ! built_from "$dir" $commit; then
    src=$data/src/openEMS-Project
    rm -rf "$src" "$dir"
    git clone -q --depth 1 --branch v0.37.0 --recurse-submodules --shallow-submodules \
      https://github.com/thliebig/openEMS-Project.git "$src"
    test "$(git -C "$src" rev-parse HEAD)" = $commit
    (cd "$src" && PATH=$env/bin:$PATH CMAKE_PREFIX_PATH=$env ./update_openEMS.sh "$dir" --python \
      --disable-GUI --skip-dep-check --python-venv-mode site --njobs "$jobs")
    rm -rf "$src"
  fi
  "$dir/venv/bin/python" -c "import CSXCAD, openEMS"
  done_with "$dir" $commit
  echo "FH_OPENEMS=$dir/venv/bin/python"
}

lines=()
skipped=()
failed=()
for solver in "${solvers[@]}"; do
  IFS='|' read -r id var name types <<<"$solver"
  if [ -n "${!var:-}" ]; then
    echo "install: $name: uses your $var=${!var}"
    skipped+=("$name: you set $var, so fh uses ${!var}")
    continue
  fi
  echo "install: $name (log: $logs/$id.log)"
  # The first failed command ends the install of the solver. Bash ignores set -e in a command
  # of an || list, so the status comes from $? with set +e.
  set +e
  (set -e; install_$id) >"$logs/$id.log" 2>&1
  status=$?
  set -e
  last=$(tail -1 "$logs/$id.log")
  if [ $status != 0 ]; then
    tail -15 "$logs/$id.log" | sed 's/^/    /' >&2
    failed+=("$name: the install failed, see $logs/$id.log. This disables: $types")
  elif [[ $last == FH_* ]]; then
    lines+=("$last")
    if [ $id = openfoam ]; then openfoam=${last#FH_OPENFOAM=}; fi
  else
    skipped+=("$name: not installed: $last. This disables: $types")
  fi
done

# Copy to a new file in the folder, then rename it. A rename does not change a file that a
# running program uses, and macOS does not stop a new program for a changed signature.
put() {
  local from=$1 to=$2 mode=$3
  cp "$from" "$to.new.$$"
  chmod "$mode" "$to.new.$$"
  mv -f "$to.new.$$" "$to"
}
update=no
if [ -e "$prefix/fh" ]; then update=yes; fi
mkdir -p "$prefix/viewer"
put "$target/release/fh" "$prefix/fh" 755
for file in fh_viewer.js fh_viewer_bg.wasm; do
  put "$target/release/viewer/$file" "$prefix/viewer/$file" 644
done
# The venv path in single quotes, with each quote in it as '\''.
printf "#!/bin/sh\nexec '%s/bin/python' \"\$@\"\n" "${venv//\'/\'\\\'\'}" >"$target/release/fh-python"
put "$target/release/fh-python" "$prefix/fh-python" 755
printf '%s\n' ${lines[@]+"${lines[@]}"} >"$data/solvers.env"
put "$data/solvers.env" "$prefix/solvers.env" 644

if [ $update = yes ]; then verb=updated; else verb=installed; fi
echo "install: $verb in $prefix:"
for file in fh fh-python solvers.env viewer/fh_viewer.js viewer/fh_viewer_bg.wasm; do
  echo "  $prefix/$file"
done
echo "install: fh-python runs the Python of $venv, with build123d: fh-python part.py"
echo "install: ${#lines[@]} solver(s) installed in $data:"
for line in ${lines[@]+"${lines[@]}"}; do
  echo "  $line"
done
for line in ${skipped[@]+"${skipped[@]}"} ${failed[@]+"${failed[@]}"}; do
  echo "install: $line"
done
if [ $update = yes ]; then
  echo "install: if a daemon runs, it keeps the earlier fh until it stops. Stop it with Ctrl+C"
fi
# True when a folder on PATH is the folder $1, also by another name (a symbolic link).
on_path() {
  local want dir IFS=:
  want=$(cd "$1" && pwd -P)
  for dir in $PATH; do
    if [ -n "$dir" ] && [ "$(cd "$dir" 2>/dev/null && pwd -P)" = "$want" ]; then return 0; fi
  done
  return 1
}
run="fh serve"
if ! on_path "$prefix"; then
  run="$prefix/fh serve"
  echo "install: $prefix is not on PATH. To run fh by its name, add this line to your shell start file (for example ~/.zshrc):"
  echo "  export PATH=\"$prefix:\$PATH\""
fi
echo "install: next, start the page and write a request in the chat:"
echo "  $run"
if [ ${#failed[@]} -gt 0 ]; then exit 1; fi
