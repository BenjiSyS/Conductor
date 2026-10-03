use crate::{
    domain::*,
    permissions::{authorize, Capability},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;

pub fn project_path(root: &Path, relative: &str, writing: bool) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let input = Path::new(relative);
    if input.is_absolute()
        || input
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(Error::Denied(
            "Use a relative path inside the project".into(),
        ));
    }
    for component in input.components() {
        if let Component::Normal(part) = component {
            let name = part.to_string_lossy();
            if name.eq_ignore_ascii_case(".git") {
                return Err(Error::Denied(
                    "Direct access to Git internals is blocked".into(),
                ));
            }
            // Apply Windows-safe names on every platform so projects remain
            // portable and a path cannot address a device or alternate stream.
            let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
            let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit());
            if name.is_empty()
                || name.ends_with(['.', ' '])
                || name
                    .chars()
                    .any(|c| c.is_control() || ":*?\"<>|".contains(c))
                || reserved
            {
                return Err(Error::Denied(
                    "Path contains a reserved or unsafe filename".into(),
                ));
            }
        }
    }
    let candidate = root.join(input);
    // Reject symlinks and junctions in every existing ancestor.
    let mut checked = root.clone();
    for component in input.components() {
        checked.push(component);
        match std::fs::symlink_metadata(&checked) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(Error::Denied("Symlink access is blocked".into()));
                }
                if !checked.canonicalize()?.starts_with(&root) {
                    return Err(Error::Denied("Path escapes project".into()));
                }
            }
            Err(e) if writing && e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    if writing && !candidate.parent().is_some_and(Path::is_dir) {
        return Err(Error::Invalid("Parent folder must exist".into()));
    }
    Ok(candidate)
}

pub fn read_file(root: &Path, path: &str) -> Result<String> {
    let path = project_path(root, path, false)?;
    if std::fs::metadata(&path)?.len() > 1_000_000 {
        return Err(Error::Invalid(
            "File exceeds 1 MB; use a targeted range".into(),
        ));
    }
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(1_000_000)
        .read_to_string(&mut text)?;
    Ok(text)
}
pub fn write_file(
    root: &Path,
    path: &str,
    text: &str,
    mode: Mode,
    settings: &Settings,
    approved: bool,
) -> Result<()> {
    authorize(mode, settings, Capability::FileWrite, approved)?;
    if text.len() > 1_000_000 {
        return Err(Error::Invalid("File exceeds 1 MB".into()));
    }
    let path = project_path(root, path, true)?;
    // create_new avoids clobbering another operation's temporary file.
    let temporary = path.with_extension(format!("conductor-{}", id()));
    let result = (|| -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        // This basic API has no checkpoint contract. Preserve existing content;
        // the integrated engine owns verified replacement and checkpoint logic.
        if path.exists() {
            return Err(Error::Invalid(
                "Existing-file replacement requires checkpoint support (not yet implemented)"
                    .into(),
            ));
        }
        std::fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if temporary.exists() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub fn open_project(path: &Path) -> Result<Project> {
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(Error::Invalid("Choose a project folder".into()));
    }
    let kind = [
        ("Cargo.toml", "Rust"),
        ("package.json", "JavaScript / TypeScript"),
        ("pyproject.toml", "Python"),
        ("project.godot", "Godot"),
        ("pom.xml", "Java"),
        ("build.gradle", "Gradle"),
        ("CMakeLists.txt", "C / C++"),
    ]
    .iter()
    .find(|(file, _)| path.join(file).exists())
    .map_or("Folder", |(_, kind)| *kind);
    Ok(Project {
        id: id(),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: path.display().to_string(),
        kind: kind.into(),
        git: path.join(".git").exists(),
        opened_at: now(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
}

async fn bounded_output<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> Result<(Vec<u8>, bool)> {
    let mut saved = Vec::new();
    let mut buf = [0u8; 8192];
    let mut truncated = false;
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let available = 256_000usize.saturating_sub(saved.len());
        saved.extend_from_slice(&buf[..n.min(available)]);
        if n > available {
            truncated = true;
        }
    }
    Ok((saved, truncated))
}

pub async fn run_command(
    root: &Path,
    program: &str,
    args: &[String],
    mode: Mode,
    settings: &Settings,
    approved: bool,
    cancel: CancellationToken,
) -> Result<CommandResult> {
    authorize(mode, settings, Capability::Terminal, approved)?;
    if program.is_empty() || program.len() > 4096 || args.len() > 128 {
        return Err(Error::Invalid("Invalid command".into()));
    }
    let root = root.canonicalize()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Invalid("No command output".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Invalid("No error output".into()))?;
    let output = async {
        let (out, err, status) =
            tokio::try_join!(bounded_output(stdout), bounded_output(stderr), async {
                child.wait().await.map_err(Error::from)
            })?;
        Ok::<_, Error>(CommandResult {
            exit_code: status.code(),
            stdout: crate::context::redact(&String::from_utf8_lossy(&out.0)),
            stderr: crate::context::redact(&String::from_utf8_lossy(&err.0)),
            truncated: out.1 || err.1,
        })
    };
    tokio::select! {
        result = output => result,
        _ = cancel.cancelled() => {let _ = child.kill().await;Err(Error::Cancelled)},
        _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => {let _ = child.kill().await;Err(Error::Invalid("Command exceeded five-minute limit".into()))},
    }
}

pub async fn git_status(root: &Path, cancel: CancellationToken) -> Result<CommandResult> {
    run_command(
        root,
        "git",
        &[
            "--no-optional-locks".into(),
            "status".into(),
            "--short".into(),
            "--branch".into(),
        ],
        Mode::Agent,
        &Settings::default(),
        true,
        cancel,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn engine_path_guard_blocks_git_aliases_before_and_after_git_init() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let guard = conductor_security::paths::PathGuard::new(temp.path())
            .map_err(|e| Error::Invalid(e.to_string()))?;
        for initialized in [false, true] {
            if initialized {
                std::fs::create_dir(temp.path().join(".git"))?;
                std::fs::write(temp.path().join(".git/config"), "fixture")?;
            }
            for path in [".git/config", ".GIT/config", ".git./config", ".git /config"] {
                assert!(
                    guard.resolve_write(path).is_err(),
                    "allowed {path} with Git initialized={initialized}"
                );
            }
        }
        Ok(())
    }
    #[test]
    fn blocks_traversal_and_preserves_existing_files() -> Result<()> {
        let temp = tempfile::tempdir()?;
        assert!(project_path(temp.path(), "../outside", true).is_err());
        assert!(project_path(temp.path(), ".git/config", true).is_err());
        for path in [
            ".GIT/config",
            "CON",
            "NUL.txt",
            "COM1.log",
            "file.txt:hidden",
            "trailing.",
            "trailing ",
            "control\u{0}.txt",
        ] {
            assert!(project_path(temp.path(), path, true).is_err(), "{path:?}");
        }
        let settings = Settings {
            permission: PermissionLevel::FullAccess,
            ..Default::default()
        };
        write_file(
            temp.path(),
            "hello.txt",
            "first",
            Mode::Agent,
            &settings,
            false,
        )?;
        assert!(write_file(
            temp.path(),
            "hello.txt",
            "second",
            Mode::Agent,
            &settings,
            false
        )
        .is_err());
        assert_eq!(read_file(temp.path(), "hello.txt")?, "first");
        assert!(write_file(temp.path(), "plan.txt", "no", Mode::Plan, &settings, true).is_err());
        Ok(())
    }
    #[tokio::test]
    async fn commands_use_argv_not_shell_interpolation() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let result = run_command(
            temp.path(),
            "git",
            &["--version".into(), "; echo injected".into()],
            Mode::Agent,
            &Settings::default(),
            true,
            CancellationToken::new(),
        )
        .await?;
        assert!(!result.stdout.contains("injected"));
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn blocks_symlink_escape() -> Result<()> {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        std::os::unix::fs::symlink(outside.path(), root.path().join("link"))?;
        assert!(project_path(root.path(), "link/a.txt", true).is_err());
        Ok(())
    }
}
