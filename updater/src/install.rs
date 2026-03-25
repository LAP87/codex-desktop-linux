use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const PACKAGE_NAME: &str = "codex-desktop";
const INSTALLED_UPDATER_BINARY: &str = "/usr/bin/codex-update-manager";

pub fn installed_package_version() -> String {
    match Command::new("dpkg-query")
        .args(["-W", "-f=${Version}", PACKAGE_NAME])
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if version.is_empty() {
                "unknown".to_string()
            } else {
                version
            }
        }
        _ => "unknown".to_string(),
    }
}

pub fn install_deb(path: &Path) -> Result<()> {
    anyhow::ensure!(path.exists(), "Debian package not found: {}", path.display());
    ensure_upgrade_path(path)?;

    if command_exists("apt") {
        let mut command = apt_install_command(path)?;
        run_install(&mut command).context("apt install failed")?;
        return Ok(());
    }

    let mut command = dpkg_install_command(path);
    run_install(&mut command).context("dpkg -i failed")
}

pub fn pkexec_command(current_exe: &Path, deb_path: &Path) -> Command {
    let updater_binary = updater_binary_for_privileged_install(current_exe);
    let mut command = Command::new("pkexec");
    command
        .arg(updater_binary)
        .arg("install-deb")
        .arg("--path")
        .arg(deb_path);
    command
}

fn run_install(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .context("Failed to execute installation command")?;
    anyhow::ensure!(status.success(), "installation command exited with {status}");
    Ok(())
}

fn ensure_upgrade_path(path: &Path) -> Result<()> {
    let installed = installed_package_version();
    if installed == "unknown" {
        return Ok(());
    }

    let candidate = deb_package_version(path)?;
    anyhow::ensure!(
        is_version_newer(&candidate, &installed)?,
        "Refusing to install non-newer package version {candidate} over installed version {installed}"
    );
    Ok(())
}

fn apt_install_command(path: &Path) -> Result<Command> {
    let parent = path
        .parent()
        .context("Debian package path has no parent directory")?;
    let file_name = path
        .file_name()
        .context("Debian package path has no file name")?
        .to_string_lossy()
        .into_owned();

    let mut command = Command::new("apt");
    command
        .current_dir(parent)
        .arg("install")
        .arg("-y")
        .arg(format!("./{file_name}"));
    Ok(command)
}

fn dpkg_install_command(path: &Path) -> Command {
    let mut command = Command::new("dpkg");
    command.arg("-i").arg(path.as_os_str());
    command
}

fn updater_binary_for_privileged_install(current_exe: &Path) -> PathBuf {
    let installed = PathBuf::from(INSTALLED_UPDATER_BINARY);
    if installed.is_file() {
        installed
    } else {
        current_exe.to_path_buf()
    }
}

fn deb_package_version(path: &Path) -> Result<String> {
    let output = Command::new("dpkg-deb")
        .arg("-f")
        .arg(path)
        .arg("Version")
        .output()
        .context("Failed to inspect Debian package metadata")?;

    anyhow::ensure!(
        output.status.success(),
        "dpkg-deb could not read the package version from {}",
        path.display()
    );

    let version = String::from_utf8(output.stdout)
        .context("dpkg-deb returned a non-UTF8 package version")?
        .trim()
        .to_string();
    anyhow::ensure!(
        !version.is_empty(),
        "dpkg-deb returned an empty package version for {}",
        path.display()
    );
    Ok(version)
}

fn is_version_newer(candidate: &str, installed: &str) -> Result<bool> {
    let status = Command::new("dpkg")
        .args(["--compare-versions", candidate, "gt", installed])
        .status()
        .context("Failed to compare Debian package versions")?;
    Ok(status.success())
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|entry| {
                let candidate: PathBuf = entry.join(name);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_pkexec_command_for_privileged_install() {
        let command = pkexec_command(
            Path::new("/usr/bin/codex-update-manager"),
            Path::new("/tmp/update.deb"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "/usr/bin/codex-update-manager",
                "install-deb",
                "--path",
                "/tmp/update.deb"
            ]
        );
    }

    #[test]
    fn prefers_installed_updater_path_for_pkexec() {
        let selected =
            updater_binary_for_privileged_install(Path::new("/tmp/codex-update-manager-old"));
        assert_eq!(selected, PathBuf::from("/usr/bin/codex-update-manager"));
    }

    #[test]
    fn builds_local_apt_install_command() -> Result<()> {
        let command = apt_install_command(Path::new("/tmp/build/codex.deb"))?;
        assert_eq!(command.get_program().to_string_lossy(), "apt");
        assert_eq!(
            command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["install", "-y", "./codex.deb"]
        );
        Ok(())
    }

    #[test]
    fn compares_debian_versions_using_dpkg_rules() -> Result<()> {
        assert!(is_version_newer(
            "2026.03.24.220000+88f07cd3",
            "2026.03.24.120000+afed8a8e"
        )?);
        assert!(!is_version_newer(
            "2026.03.24.120000+88f07cd3",
            "2026.03.24.120000+afed8a8e"
        )?);
        Ok(())
    }
}
