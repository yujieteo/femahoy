//! Open the page of the daemon in the browser of the user (DESIGN.md section 4.1). `fh serve`
//! does this by itself. The environment variable `FH_NO_OPEN` with a value that is not empty
//! and not `0` stops it, for tests and for a machine with no screen.
//!
//! The address of the page holds the token. The token must not stay in the command line of a
//! browser process, because other users of the machine can read a command line:
//!
//! - `$BROWSER`, when it is set, and `xdg-open` (Unix systems other than macOS) get the file
//!   `<state>/open.html` (mode 0600). The file sends the browser on to the address.
//! - On macOS, `open` gives the address to the default browser through Launch Services. Only
//!   the short-lived `open` process has the address in its command line.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

/// The environment variable that stops the browser open.
pub const NO_OPEN: &str = "FH_NO_OPEN";

/// The file in the state folder that sends the browser on to the page.
pub const OPEN_FILE: &str = "open.html";

/// True when the browser must not open: `FH_NO_OPEN` has a value that is not empty and not `0`.
pub fn no_open() -> bool {
    std::env::var_os(NO_OPEN).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Open `url`, an address of the page of the daemon of `state_dir`, in the browser. This does
/// not wait for the browser. An error tells why no browser program could start.
pub fn open_page(state_dir: &Path, url: &str) -> Result<(), String> {
    let mut args: Vec<OsString> = Vec::new();
    let program: OsString = match std::env::var_os("BROWSER").filter(|b| !b.is_empty()) {
        Some(browser) => {
            args.push(redirect(state_dir, url)?);
            browser
        }
        None if cfg!(target_os = "macos") => {
            args.push(url.into());
            "open".into()
        }
        None => {
            args.push(redirect(state_dir, url)?);
            "xdg-open".into()
        }
    };
    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", program.to_string_lossy()))?;
    // Collect the exit status, so that the program does not stay as a zombie process.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Write `<state>/open.html`, a page that sends the browser on to `url`. Only the user can
/// read it, because it holds the token.
fn redirect(state_dir: &Path, url: &str) -> Result<OsString, String> {
    let attr = url
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    let html = format!(
        "<!doctype html>\n<meta charset=\"utf-8\">\n<meta name=\"referrer\" content=\"no-referrer\">\n\
         <meta http-equiv=\"refresh\" content=\"0; url={attr}\">\n<title>femahoy</title>\n\
         <p><a href=\"{attr}\">Open femahoy</a></p>\n"
    );
    let path = state_dir.join(OPEN_FILE);
    crate::write_private(&path, &html).map_err(|e| e.to_string())?;
    Ok(path.into_os_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_redirect_page_escapes_the_address_and_only_the_user_can_read_it() {
        let dir = std::env::temp_dir().join(format!("fh-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = redirect(&dir, "http://127.0.0.1:1/?token=ab&run=run-000001");
        let text = std::fs::read_to_string(file.unwrap()).unwrap();
        assert!(text.contains("url=http://127.0.0.1:1/?token=ab&amp;run=run-000001\""));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join(OPEN_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(dir);
    }
}
