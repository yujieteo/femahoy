//! Open the page of the daemon in the browser of the user (DESIGN.md section 4.1). `fh serve`
//! does this by itself on macOS. On other systems it opens no browser and only prints the
//! address. The environment variable `FH_NO_OPEN` with a value that is not empty and not `0`
//! stops the open, for tests and for a machine with no screen.
//!
//! The address of the page holds the token. The token must not stay in the command line of a
//! browser process, because other users of the machine can read a command line. On macOS,
//! `open` gives the address to the default browser through Launch Services. Only the
//! short-lived `open` process has the address in its command line.

use std::process::{Command, Stdio};

/// The environment variable that stops the browser open.
pub const NO_OPEN: &str = "FH_NO_OPEN";

/// True when the browser must not open: `FH_NO_OPEN` has a value that is not empty and not `0`.
pub fn no_open() -> bool {
    std::env::var_os(NO_OPEN).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Open `url`, the address of the page of the daemon, in the browser. This does not wait for
/// the browser. An error tells why no browser could start.
pub fn open_page(url: &str) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("only macOS opens a browser".into());
    }
    let mut child = Command::new("open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start open: {e}"))?;
    // Collect the exit status, so that the program does not stay as a zombie process.
    std::thread::spawn(move || child.wait());
    Ok(())
}
