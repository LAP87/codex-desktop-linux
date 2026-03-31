//! Installation helpers for privileged and non-privileged package application.

use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const PACKAGE_NAME: &str = "codex-desktop";
const INSTALLED_UPDATER_BINARY: &str = "/usr/bin/codex-update-manager";
const APT_CANDIDATES: &[&str] = &["/usr/bin/apt", "/bin/apt"];
const DNF_CANDIDATES: &[&str] = &["/usr/bin/dnf", "/bin/dnf", "/usr/bin/dnf5", "/bin/dnf5"];
const DPKG_CANDIDATES: &[&str] = &["/usr/bin/dpkg", "/bin/dpkg"];
const DPKG_DEB_CANDIDATES: &[&str] = &["/usr/bin/dpkg-deb", "/bin/dpkg-deb"];
const DPKG_QUERY_CANDIDATES: &[&str] = &["/usr/bin/dpkg-query", "/bin/dpkg-query"];
const RPM_CANDIDATES: &[&str] = &["/usr/bin/rpm", "/bin/rpm"];
const PACMAN_CANDIDATES: &[&str] = &["/usr/bin/pacman", "/bin/pacman"];

/// The native package format in use on the current system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Deb,
    Rpm,
    Pacman,
}

impl PackageKind {
    /// Detect the package manager available on the running system.
    /// Checks pacman first (Arch), then dpkg (Debian/Ubuntu), then rpm (Fedora).
    pub fn detect() -> Self {
        if program_exists(PACMAN_CANDIDATES, "pacman") {
            Self::Pacman
        } else if program_exists(DPKG_CANDIDATES, "dpkg") {
            Self::Deb
        } else if program_exists(RPM_CANDIDATES, "rpm") {
            Self::Rpm
        } else {
            Self::Deb
        }
    }

    /// Infer the package kind from a file path extension.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rpm") => Self::Rpm,
            _ => Self::Deb,
        }
    }
}

/// Returns the currently installed package version when available.
pub fn installed_package_version() -> String {
    match PackageKind::detect() {
        PackageKind::Deb => installed_deb_version(),
        PackageKind::Rpm => installed_rpm_version(),
        PackageKind::Pacman => installed_pacman_version(),
    }
}

/// Returns whether the primary native package still appears to be installed.
pub fn is_primary_package_installed() -> bool {
    installed_package_version() != "unknown"
}

fn installed_deb_version() -> String {
    installed_version_from_command(
        &program_path(DPKG_QUERY_CANDIDATES, "dpkg-query"),
        &["-W", "-f=${Version}", PACKAGE_NAME],
    )
}

fn installed_rpm_version() -> String {
    installed_version_from_command(
        &program_path(RPM_CANDIDATES, "rpm"),
        &["-q", "--queryformat", "%{VERSION}-%{RELEASE}", PACKAGE_NAME],
    )
}

fn installed_pacman_version() -> String {
    installed_version_from_command(
        &program_path(PACMAN_CANDIDATES, "pacman"),
        &["-Q", PACKAGE_NAME],
    )
}

/// Installs a rebuilt Debian package on the local machine.
pub fn install_deb(path: &Path) -> Result<()> {
    anyhow::ensure!(
        path.exists(),
        "Debian package not found: {}",
        path.display()
    );
    ensure_upgrade_path(path)?;

    if program_exists(APT_CANDIDATES, "apt") {
        let mut command = apt_install_command(path)?;
        run_install(&mut command).context("apt install failed")?;
        return Ok(());
    }

    let mut command = dpkg_install_command(path);
    run_install(&mut command).context("dpkg -i failed")
}

/// Installs a rebuilt RPM package on the local machine.
pub fn install_rpm(path: &Path) -> Result<()> {
    anyhow::ensure!(path.exists(), "RPM package not found: {}", path.display());

    if program_exists(DNF_CANDIDATES, "dnf") || program_exists(DNF_CANDIDATES, "dnf5") {
        let mut command = dnf_install_command(path)?;
        run_install(&mut command).context("dnf install failed")?;
        return Ok(());
    }

    let mut command = rpm_install_command(path);
    run_install(&mut command).context("rpm -Uvh failed")
}

/// Installs a package via pacman on Arch Linux / CachyOS.
pub fn install_pacman(path: &Path) -> Result<()> {
    anyhow::ensure!(
        path.exists(),
        "Pacman package not found: {}",
        path.display()
    );
    ensure_upgrade_path_pacman(path)?;

    let mut command = pacman_install_command(path);
    run_install(&mut command).context("pacman -U failed")
}

/// Builds the `pkexec` command used for privileged package installation.
pub fn pkexec_command(current_exe: &Path, package_path: &Path) -> Command {
    let updater_binary = updater_binary_for_privileged_install(current_exe);
    let subcommand = match PackageKind::from_path(package_path) {
        PackageKind::Rpm => "install-rpm",
        PackageKind::Deb => "install-deb",
        PackageKind::Pacman => "install-pacman",
    };
    let mut command = Command::new("pkexec");
    command
        .arg(updater_binary)
        .arg(subcommand)
        .arg("--path")
        .arg(package_path);
    command
}

fn run_install(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .context("Failed to execute installation command")?;
    anyhow::ensure!(
        status.success(),
        "installation command exited with {status}"
    );
    Ok(())
}

fn installed_version_from_command(program: &Path, args: &[&str]) -> String {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => parse_installed_version(output.stdout),
        _ => "unknown".to_string(),
    }
}

fn parse_installed_version(stdout: Vec<u8>) -> String {
    let version = String::from_utf8_lossy(&stdout).trim().to_string();
    if version.is_empty() {
        "unknown".to_string()
    } else {
        version
    }
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

fn ensure_upgrade_path_pacman(path: &Path) -> Result<()> {
    let installed = installed_pacman_version();
    if installed == "unknown" {
        return Ok(());
    }

    let candidate = pacman_package_version(path)?;
    anyhow::ensure!(
        is_version_newer_pacman(&candidate, &installed)?,
        "Refusing to install non-newer package version {candidate} over installed version {installed}"
    );
    Ok(())
}

fn apt_install_command(path: &Path) -> Result<Command> {
    install_command_in_parent(&program_path(APT_CANDIDATES, "apt"), path)
}

fn dpkg_install_command(path: &Path) -> Command {
    let mut command = Command::new(program_path(DPKG_CANDIDATES, "dpkg"));
    command.arg("-i").arg(path.as_os_str());
    command
}

fn dnf_install_command(path: &Path) -> Result<Command> {
    install_command_in_parent(&program_path(DNF_CANDIDATES, "dnf"), path)
}

fn install_command_in_parent(program: &Path, path: &Path) -> Result<Command> {
    let program_name = program
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("package manager");
    let parent = path
        .parent()
        .with_context(|| format!("{program_name} package path has no parent directory"))?;
    let file_name = path
        .file_name()
        .with_context(|| format!("{program_name} package path has no file name"))?
        .to_string_lossy()
        .into_owned();

    let mut command = Command::new(program);
    command
        .current_dir(parent)
        .arg("install")
        .arg("-y")
        .arg(format!("./{file_name}"));
    Ok(command)
}

fn rpm_install_command(path: &Path) -> Command {
    let mut command = Command::new(program_path(RPM_CANDIDATES, "rpm"));
    command.args(["-Uvh"]).arg(path.as_os_str());
    command
}

fn pacman_install_command(path: &Path) -> Command {
    let mut command = Command::new(program_path(PACMAN_CANDIDATES, "pacman"));
    command.arg("-U").arg(path.as_os_str());
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
    let output = Command::new(program_path(DPKG_DEB_CANDIDATES, "dpkg-deb"))
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
    let status = Command::new(program_path(DPKG_CANDIDATES, "dpkg"))
        .args(["--compare-versions", candidate, "gt", installed])
        .status()
        .context("Failed to compare Debian package versions")?;
    Ok(status.success())
}

fn pacman_package_version(path: &Path) -> Result<String> {
    // Parse pacman package version from filename.
    // Format: codex-desktop-1.0.0-1-x86_64.pkg.tar.zst
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Package path has no file name")?;

    // Strip extension (.pkg.tar.zst, .pkg.tar.xz, .pkg.tar.gz, etc.)
    let name_lower = file_name.to_lowercase();
    let stripped = name_lower
        .strip_suffix(".pkg.tar.zst")
        .or_else(|| name_lower.strip_suffix(".pkg.tar.xz"))
        .or_else(|| name_lower.strip_suffix(".pkg.tar.gz"))
        .or_else(|| name_lower.strip_suffix(".pkg.tar.bz2"))
        .or_else(|| name_lower.strip_suffix(".pkg.tar.lz"))
        .or_else(|| name_lower.strip_suffix(".pkg.tar.lz4"))
        .or_else(|| name_lower.strip_suffix(".pkg.tar.lz5"))
        .or_else(|| name_lower.strip_suffix(".pkg.tar.zst"))
        .context("Not a valid pacman package filename")?;

    // Extract name-version-release from: codex-desktop-1.0.0-1-x86_64
    // The format is: name-version-release-arch
    let parts: Vec<&str> = stripped.rsplitn(3, '-').collect();
    if parts.len() < 3 {
        anyhow::bail!("Could not parse package version from: {}", file_name);
    }
    // parts[0] = arch, parts[1] = release, parts[2] = version... but name might have dashes too
    // Use a simpler approach: find the last two dash-separated numeric parts
    let version_release = format!("{}-{}", parts[2], parts[1]);
    Ok(version_release)
}

fn is_version_newer_pacman(candidate: &str, _installed: &str) -> Result<bool> {
    // Parse version-release from candidate filename
    // Format: name-version-release-arch (e.g. codex-desktop-1.2.3-1-x86_64)
    let parts: Vec<&str> = candidate.split('-').collect();
    if parts.len() < 3 {
        return Ok(false);
    }
    // Last two parts are version and release
    let cand_ver = parts[parts.len() - 2];
    let cand_rel = parts[parts.len() - 1];

    // Compare against currently installed via pacman -Q
    let output = Command::new(program_path(PACMAN_CANDIDATES, "pacman"))
        .args(["-Q", "codex-desktop"])
        .output()
        .context("Failed to query installed pacman version")?;

    if !output.status.success() {
        // Not installed, allow install
        return Ok(true);
    }

    let installed_full = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let inst_parts: Vec<&str> = installed_full.split('-').collect();
    if inst_parts.len() < 3 {
        return Ok(true);
    }
    let inst_ver = inst_parts[inst_parts.len() - 2];
    let inst_rel = inst_parts[inst_parts.len() - 1];

    // Compare version first, then release
    if cand_ver != inst_ver {
        return Ok(cand_ver > inst_ver);
    }
    Ok(cand_rel > inst_rel)
}

fn program_exists(candidates: &[&str], fallback: &str) -> bool {
    candidates.iter().any(|path| Path::new(path).is_file()) || command_exists(fallback)
}

fn program_path(candidates: &[&str], fallback: &str) -> PathBuf {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(fallback))
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
    use anyhow::Result;

    #[test]
    fn builds_pkexec_command_for_privileged_deb_install() {
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
    fn builds_pkexec_command_for_privileged_rpm_install() {
        let command = pkexec_command(
            Path::new("/usr/bin/codex-update-manager"),
            Path::new("/tmp/update.rpm"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "/usr/bin/codex-update-manager",
                "install-rpm",
                "--path",
                "/tmp/update.rpm"
            ]
        );
    }

    #[test]
    fn prefers_installed_updater_path_for_pkexec() {
        let selected =
            updater_binary_for_privileged_install(Path::new("/tmp/codex-update-manager-old"));
        let expected = if Path::new("/usr/bin/codex-update-manager").is_file() {
            PathBuf::from("/usr/bin/codex-update-manager")
        } else {
            PathBuf::from("/tmp/codex-update-manager-old")
        };
        assert_eq!(selected, expected);
    }

    #[test]
    fn builds_local_apt_install_command() -> Result<()> {
        let command = apt_install_command(Path::new("/tmp/build/codex.deb"))?;
        assert!(command.get_program().to_string_lossy().ends_with("apt"));
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
    fn builds_local_dnf_install_command() -> Result<()> {
        let command = dnf_install_command(Path::new("/tmp/build/codex.rpm"))?;
        let program = command.get_program().to_string_lossy();
        assert!(program.ends_with("dnf") || program.ends_with("dnf5"));
        assert_eq!(
            command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["install", "-y", "./codex.rpm"]
        );
        Ok(())
    }

    #[test]
    fn package_kind_from_path_detects_rpm() {
        assert_eq!(
            PackageKind::from_path(Path::new("/tmp/codex.rpm")),
            PackageKind::Rpm
        );
    }

    #[test]
    fn package_kind_from_path_detects_deb() {
        assert_eq!(
            PackageKind::from_path(Path::new("/tmp/codex.deb")),
            PackageKind::Deb
        );
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

    #[test]
    fn install_commands_require_a_file_name() {
        let deb_error = apt_install_command(Path::new("/")).expect_err("root is not a package");
        let rpm_error = dnf_install_command(Path::new("/")).expect_err("root is not a package");

        assert!(deb_error.to_string().contains("apt package path has no"));
        assert!(rpm_error.to_string().contains("dnf package path has no"));
    }

    #[test]
    fn empty_installed_version_output_is_reported_as_unknown() {
        assert_eq!(parse_installed_version(Vec::new()), "unknown");
    }
}
