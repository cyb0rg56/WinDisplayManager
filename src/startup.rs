//! Per-user "start with Windows" registration.
//!
//! The sign-in command lives in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! `--minimized` is added only when both startup toggles are on, so a normal launch
//! always opens the window.

use std::io;
use std::path::Path;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "WindowsDisplayManager";
pub const MINIMIZED_ARG: &str = "--minimized";

/// True when this process was started by the sign-in command with the minimized flag.
pub fn launched_minimized() -> bool {
    std::env::args().any(|arg| arg == MINIMIZED_ARG)
}

/// Quoted executable path, plus `--minimized` when requested.
pub fn startup_command(exe: &Path, minimized: bool) -> String {
    let quoted = quote_path(exe);
    if minimized {
        format!("{quoted} {MINIMIZED_ARG}")
    } else {
        quoted
    }
}

/// Write or remove the Run value so it matches the saved startup settings.
///
/// A missing value while startup is off is success. The executable path is the
/// current process, so moving the install updates the registration on next launch.
pub fn apply(enabled: bool, minimized: bool) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    set_run_value(&exe, enabled, minimized)
}

fn quote_path(exe: &Path) -> String {
    let path = exe.to_string_lossy().replace('"', "\\\"");
    format!("\"{path}\"")
}

fn set_run_value(exe: &Path, enabled: bool, minimized: bool) -> io::Result<()> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN_KEY)?;
    if !enabled {
        return delete_run_value(&run);
    }
    run.set_value(VALUE_NAME, &startup_command(exe, minimized))
}

fn delete_run_value(run: &winreg::RegKey) -> io::Result<()> {
    match run.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_quotes_paths_and_adds_the_minimized_flag_only_when_asked() {
        let exe = Path::new(r"C:\Program Files\Windows Display Manager\windisplaymanager_rs.exe");
        assert_eq!(
            startup_command(exe, false),
            r#""C:\Program Files\Windows Display Manager\windisplaymanager_rs.exe""#
        );
        assert_eq!(
            startup_command(exe, true),
            r#""C:\Program Files\Windows Display Manager\windisplaymanager_rs.exe" --minimized"#
        );
    }
}
