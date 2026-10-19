//! `scripts/install.sh` with fake tools: the checks before the build, the install folder, and
//! the update. The fake cargo, wasm-bindgen and uv write small files in place of a build and a
//! venv. Each test has its own HOME, so the default prefix and venv are in the scratch folder.
mod common;
use common::{p, root, scratch, script, stderr, stdout};

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The value in `"..."` on the first line of `file` that starts with `key`.
fn pinned(file: &str, key: &str) -> String {
    let text = fs::read_to_string(root().join(file)).unwrap();
    let mut lines = text.lines();
    let line = match key {
        // The version line follows the name line in Cargo.lock.
        "wasm-bindgen" => lines
            .by_ref()
            .find(|l| *l == "name = \"wasm-bindgen\"")
            .and(lines.next()),
        _ => lines.find(|l| l.starts_with(key)),
    };
    line.unwrap().split('"').nth(1).unwrap().to_owned()
}

/// A scratch HOME, a folder of fake tools, and the install command.
struct Lab {
    dir: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    target: PathBuf,
    toolchain: String,
    bindgen: String,
}

impl Lab {
    fn new(name: &str) -> Lab {
        let dir = scratch(name);
        let lab = Lab {
            home: dir.join("home"),
            bin: dir.join("bin"),
            target: dir.join("target"),
            dir,
            toolchain: pinned("rust-toolchain.toml", "channel"),
            bindgen: pinned("Cargo.lock", "wasm-bindgen"),
        };
        fs::create_dir_all(&lab.home).unwrap();
        fs::create_dir_all(&lab.bin).unwrap();
        lab
    }

    /// A fake cargo in `folder`. It logs its folder, RUSTUP_AUTO_INSTALL and its arguments.
    fn cargo(&self, folder: &Path) {
        let log = p(&self.dir.join("cargo.log")).to_owned();
        script(
            &folder.join("cargo"),
            &format!(
                "#!/bin/sh\necho \"$PWD $RUSTUP_AUTO_INSTALL $*\" >> {log}\n\
                 case \"$*\" in\n\
                 *wasm32-unknown-unknown*) d=$CARGO_TARGET_DIR/wasm32-unknown-unknown/release\n\
                   mkdir -p $d && echo wasm > $d/fh_viewer.wasm ;;\n\
                 *) mkdir -p $CARGO_TARGET_DIR/release\n\
                   for b in fh fh-solve-ccx; do printf '#!/bin/sh\\necho %s\\n' $b > $CARGO_TARGET_DIR/release/$b; done ;;\n\
                 esac\n"
            ),
        );
    }

    /// A fake rustup with the toolchain `toolchain` (empty: none) and the `installed` lines
    /// of `rustup component list --installed`.
    fn rustup(&self, toolchain: &str, installed: &[&str]) {
        script(
            &self.bin.join("rustup"),
            &format!(
                "#!/bin/sh\ncase \"$1 $2\" in\n\
                 'toolchain list') [ -n '{toolchain}' ] && echo '{toolchain}-aarch64-apple-darwin (default)' ;;\n\
                 'component list') printf '{}' ;;\n\
                 *) exit 1 ;;\nesac\nexit 0\n",
                installed.iter().map(|l| format!("{l}\\n")).collect::<String>()
            ),
        );
    }

    fn wasm_bindgen(&self, version: &str) {
        script(
            &self.bin.join("wasm-bindgen"),
            &format!(
                "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'wasm-bindgen {version}'; exit 0; fi\n\
                 while [ $# -gt 1 ]; do if [ \"$1\" = --out-dir ]; then out=$2; fi; shift; done\n\
                 echo js > $out/fh_viewer.js && cp \"$1\" $out/fh_viewer_bg.wasm\n"
            ),
        );
    }

    /// A fake uv. It logs its arguments, and `uv venv DIR` writes a fake DIR/bin/python.
    fn uv(&self) {
        let log = p(&self.dir.join("uv.log")).to_owned();
        script(
            &self.bin.join("uv"),
            &format!(
                "#!/bin/sh\necho \"$*\" >> {log}\n\
                 if [ \"$1\" = venv ]; then for d; do :; done; mkdir -p \"$d/bin\"\n\
                   printf '#!/bin/sh\\necho python \"$@\"\\n' > \"$d/bin/python\"; chmod 755 \"$d/bin/python\"; fi\n"
            ),
        );
    }

    /// All the tools, with cargo in HOME/.cargo/bin, which is not on PATH.
    fn all_tools(&self) {
        self.cargo(&self.home.join(".cargo/bin"));
        let installed = [
            "cargo-aarch64-apple-darwin",
            "clippy-aarch64-apple-darwin",
            "rust-std-wasm32-unknown-unknown",
            "rustfmt-aarch64-apple-darwin",
        ];
        self.rustup(&self.toolchain.clone(), &installed);
        self.wasm_bindgen(&self.bindgen.clone());
        self.uv();
    }

    /// `scripts/install.sh ARGS` with only the fake tools and the system folders on PATH.
    fn install(&self, args: &[&str], path: &[&Path]) -> Output {
        let path: Vec<&Path> = [
            &[self.bin.as_path()][..],
            path,
            &[Path::new("/usr/bin"), Path::new("/bin")],
        ]
        .concat();
        Command::new(root().join("scripts/install.sh"))
            .args(args)
            .current_dir(&self.dir)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("CARGO_TARGET_DIR", &self.target)
            // No ccx, also on a machine that has one in /usr/bin.
            .env("FH_CCX", self.dir.join("no-ccx"))
            .output()
            .unwrap()
    }
}

#[test]
fn missing_tools_are_listed_with_their_install_commands_before_a_build() {
    if Path::new("/usr/bin/cargo").exists() {
        eprintln!("skipped: /usr/bin/cargo exists, so cargo cannot be missing");
        return;
    }
    let lab = Lab::new("install-none");
    let out = lab.install(&[], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("tool(s) are missing"), "{err}");
    assert!(err.contains("https://sh.rustup.rs"), "{err}");
    let bindgen = format!(
        "cargo install wasm-bindgen-cli --version {} --locked",
        lab.bindgen
    );
    assert!(err.contains(&bindgen), "{err}");
    assert!(err.contains("https://astral.sh/uv/install.sh"), "{err}");
    assert!(!lab.target.exists() && !lab.home.join(".local").exists());
}

#[test]
fn a_missing_toolchain_target_component_or_bindgen_version_is_listed() {
    let lab = Lab::new("install-partial");
    lab.cargo(&lab.bin);
    lab.rustup("", &[]);
    lab.wasm_bindgen("0.2.1");
    let out = lab.install(&[], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let t = &lab.toolchain;
    let err = stderr(&out);
    assert!(
        err.contains(&format!(
            "rustup toolchain install {t} --profile minimal --component rustfmt,clippy \
         --target wasm32-unknown-unknown"
        )),
        "{err}"
    );
    assert!(err.contains("(found: 0.2.1)"), "{err}");

    lab.rustup(t, &["rustfmt-aarch64-apple-darwin"]);
    let out = lab.install(&[], &[]);
    let err = stderr(&out);
    assert!(
        err.contains(&format!("rustup component add clippy --toolchain {t}")),
        "{err}"
    );
    assert!(
        err.contains(&format!(
            "rustup target add wasm32-unknown-unknown --toolchain {t}"
        )),
        "{err}"
    );
    assert!(!err.contains("component add rustfmt"), "{err}");

    // A Rust with no rustup: the script looks for the target in the sysroot.
    fs::remove_file(lab.bin.join("rustup")).unwrap();
    let sysroot = lab.dir.join("sysroot");
    fs::create_dir_all(sysroot.join("lib/rustlib/aarch64-apple-darwin")).unwrap();
    script(
        &lab.bin.join("rustc"),
        &format!("#!/bin/sh\necho {}\n", p(&sysroot)),
    );
    let err = stderr(&lab.install(&[], &[]));
    assert!(
        err.contains("the wasm32-unknown-unknown target of your Rust: with rustup"),
        "{err}"
    );
    assert!(
        !lab.dir.join("cargo.log").exists(),
        "no build before the checks pass"
    );
}

#[test]
fn the_install_builds_copies_and_then_updates() {
    let lab = Lab::new("install-default");
    lab.all_tools();
    let out = lab.install(&[], &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}\n{}",
        stdout(&out),
        stderr(&out)
    );
    let prefix = lab.home.join(".local/bin");
    let text = stdout(&out);
    assert!(
        text.contains(&format!("install: installed in {}:", p(&prefix))),
        "{text}"
    );
    for file in [
        "fh",
        "fh-python",
        "fh-solve-ccx",
        "viewer/fh_viewer.js",
        "viewer/fh_viewer_bg.wasm",
    ] {
        assert!(prefix.join(file).is_file(), "{file}");
    }
    let mode = fs::metadata(prefix.join("fh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
    // The prefix is not on PATH, so the next command has the full path.
    assert!(
        text.contains(&format!("  {}/fh serve\n", p(&prefix))),
        "{text}"
    );
    assert!(
        text.contains(&format!("export PATH=\"{}:$PATH\"", p(&prefix))),
        "{text}"
    );
    // fh-python runs the Python 3.12 of the venv, with the tested build123d.
    let venv = lab.home.join(".local/share/femahoy/venv");
    let run = Command::new(prefix.join("fh-python"))
        .args(["part.py", "a b"])
        .output()
        .unwrap();
    assert_eq!(stdout(&run), "python part.py a b\n");
    let uv = fs::read_to_string(lab.dir.join("uv.log")).unwrap();
    assert!(
        uv.contains(&format!(
            "venv --quiet --allow-existing -p 3.12 {}",
            p(&venv)
        )),
        "{uv}"
    );
    assert!(
        uv.contains(&format!(
            "pip install --quiet -p {}/bin/python build123d==0.13.0",
            p(&venv)
        )),
        "{uv}"
    );
    // cargo ran in the repository, where rustup reads rust-toolchain.toml, and with no
    // automatic install.
    let log = fs::read_to_string(lab.dir.join("cargo.log")).unwrap();
    let repo = fs::canonicalize(root()).unwrap();
    for line in log.lines() {
        assert!(
            line.starts_with(&format!("{} 0 build ", p(&repo))),
            "{line}"
        );
    }
    assert!(
        log.contains("--release --locked --bin fh --bin fh-solve-ccx"),
        "{log}"
    );

    // FH_CCX names no file: the install says how to get ccx.
    assert!(
        text.contains("install: ccx (CalculiX) is not installed"),
        "{text}"
    );

    // The update removes the fh-solve of an earlier install.
    fs::write(prefix.join("fh"), "old").unwrap();
    fs::write(prefix.join("fh-solve"), "old").unwrap();
    let again = lab.install(&[], &[]);
    assert_eq!(again.status.code(), Some(0), "{}", stderr(&again));
    assert!(
        stdout(&again).contains("install: updated in "),
        "{}",
        stdout(&again)
    );
    assert!(stdout(&again).contains("Stop it with: fh serve --stop"));
    assert_ne!(fs::read_to_string(prefix.join("fh")).unwrap(), "old");
    let left: Vec<_> = fs::read_dir(&prefix)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left.len(), 4, "no temporary files: {left:?}");
    // The second run updates the venv too.
    let uv_again = fs::read_to_string(lab.dir.join("uv.log")).unwrap();
    assert_eq!(uv_again, format!("{uv}{uv}"));
}

#[test]
fn a_relative_prefix_on_path_gives_the_short_next_command() {
    let lab = Lab::new("install-prefix");
    lab.all_tools();
    let prefix = lab.dir.join("tools");
    let out = lab.install(&["--prefix", "tools", "--venv=env"], &[&prefix]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(prefix.join("fh").is_file() && prefix.join("viewer/fh_viewer_bg.wasm").is_file());
    assert!(lab.dir.join("env/bin/python").is_file());
    let text = stdout(&out);
    assert!(text.ends_with("\n  fh serve\n"), "{text}");
    assert!(!text.contains("export PATH"), "{text}");
}

#[test]
fn bad_install_arguments_exit_2() {
    let lab = Lab::new("install-usage");
    for args in [
        &["--prefix"][..],
        &["--bogus"],
        &["--prefix", ""],
        &["--venv"],
    ] {
        let out = lab.install(args, &[]);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(stderr(&out).contains("usage: scripts/install.sh [--prefix DIR] [--venv DIR]"));
    }
}
