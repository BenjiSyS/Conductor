//! Safe Git integration via the `git` CLI.
//!
//! Respecting user work is the rule: this module exposes no `reset --hard`,
//! `clean`, `checkout -- .`, force-push or stash-drop. Operations that could
//! overwrite uncommitted changes refuse when the tree is dirty.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::exec::{self, ExecRequest, ExecResult};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("not a Git repository")]
    NotRepo,
    #[error("git is not installed")]
    Missing,
    #[error("refusing: {0}")]
    Refused(String),
    #[error("git {cmd} failed: {stderr}")]
    Failed { cmd: String, stderr: String },
}

#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: Vec<String>,
    pub modified: Vec<String>,
    pub untracked: Vec<String>,
    pub conflicted: Vec<String>,
    pub head: Option<String>,
}

impl Status {
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty()
            && self.modified.is_empty()
            && self.untracked.is_empty()
            && self.conflicted.is_empty()
    }
    /// All paths with uncommitted changes.
    pub fn changed(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .staged
            .iter()
            .chain(&self.modified)
            .chain(&self.untracked)
            .chain(&self.conflicted)
            .cloned()
            .collect();
        v.sort();
        v.dedup();
        v
    }
    pub fn summary(&self) -> String {
        format!(
            "branch {} ({} staged, {} modified, {} untracked{})",
            self.branch.as_deref().unwrap_or("detached"),
            self.staged.len(),
            self.modified.len(),
            self.untracked.len(),
            if self.conflicted.is_empty() {
                String::new()
            } else {
                format!(", {} conflicted", self.conflicted.len())
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MergeOutcome {
    Merged,
    UpToDate,
    Conflicts(Vec<String>),
}

impl Git {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn run(&self, args: &[&str]) -> Result<ExecResult, GitError> {
        self.run_env(args, &[]).await
    }

    async fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Result<ExecResult, GitError> {
        let mut req = ExecRequest::new("git", args, &self.root).timeout(120);
        req.env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
        req.env.insert("LC_ALL".into(), "C".into());
        for (k, v) in env {
            req.env.insert(k.to_string(), v.to_string());
        }
        req.max_output_bytes = 4 * 1024 * 1024;
        let r = exec::run(req, CancellationToken::new()).await;
        if r.end == exec::ExecEnd::SpawnFailed {
            return Err(GitError::Missing);
        }
        Ok(r)
    }

    async fn ok(&self, args: &[&str]) -> Result<String, GitError> {
        self.ok_env(args, &[]).await
    }

    async fn ok_env(&self, args: &[&str], env: &[(&str, &str)]) -> Result<String, GitError> {
        let r = self.run_env(args, env).await?;
        if r.success() {
            Ok(r.stdout)
        } else if r.stderr.contains("not a git repository") {
            Err(GitError::NotRepo)
        } else {
            Err(GitError::Failed {
                cmd: args.first().unwrap_or(&"").to_string(),
                stderr: r.stderr.trim().to_string(),
            })
        }
    }

    pub async fn is_repo(&self) -> bool {
        self.ok(&["rev-parse", "--is-inside-work-tree"])
            .await
            .is_ok_and(|s| s.trim() == "true")
    }

    pub async fn init(&self) -> Result<(), GitError> {
        self.ok(&["init"]).await.map(|_| ())
    }

    pub async fn status(&self) -> Result<Status, GitError> {
        let out = self
            .ok(&[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
            ])
            .await?;
        Ok(parse_status_v2(&out))
    }

    pub async fn diff(&self, staged: bool, paths: &[&str]) -> Result<String, GitError> {
        let mut args = vec!["diff", "--no-color", "--no-ext-diff"];
        if staged {
            args.push("--cached");
        }
        args.push("--");
        args.extend_from_slice(paths);
        self.ok(&args).await
    }

    pub async fn log(&self, n: usize) -> Result<Vec<Commit>, GitError> {
        let count = format!("-{n}");
        let out = self
            .ok(&[
                "log",
                &count,
                "--pretty=format:%H%x1f%an%x1f%ad%x1f%s",
                "--date=iso-strict",
            ])
            .await?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let p: Vec<&str> = l.split('\x1f').collect();
                (p.len() == 4).then(|| Commit {
                    hash: p[0].into(),
                    author: p[1].into(),
                    date: p[2].into(),
                    subject: p[3].into(),
                })
            })
            .collect())
    }

    pub async fn branches(&self) -> Result<Vec<String>, GitError> {
        let out = self.ok(&["branch", "--format=%(refname:short)"]).await?;
        Ok(out
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }

    pub async fn current_branch(&self) -> Result<Option<String>, GitError> {
        let out = self.ok(&["branch", "--show-current"]).await?;
        let b = out.trim();
        Ok((!b.is_empty()).then(|| b.to_string()))
    }

    /// Create (and optionally switch to) a branch. Switching keeps local
    /// changes — git itself refuses if they would be overwritten.
    pub async fn create_branch(&self, name: &str, switch: bool) -> Result<(), GitError> {
        validate_ref(name)?;
        if switch {
            self.ok(&["switch", "-c", name]).await.map(|_| ())
        } else {
            self.ok(&["branch", name]).await.map(|_| ())
        }
    }

    /// Stage the given paths (never `-A` implicitly) and commit.
    pub async fn commit(&self, message: &str, paths: &[&str]) -> Result<String, GitError> {
        if message.trim().is_empty() {
            return Err(GitError::Refused("empty commit message".into()));
        }
        if paths.is_empty() {
            return Err(GitError::Refused("no paths given to commit".into()));
        }
        let mut add = vec!["add", "--"];
        add.extend_from_slice(paths);
        self.ok(&add).await?;
        self.ok(&["commit", "-m", message]).await?;
        Ok(self.ok(&["rev-parse", "HEAD"]).await?.trim().to_string())
    }

    pub async fn stash_list(&self) -> Result<Vec<String>, GitError> {
        Ok(self
            .ok(&["stash", "list"])
            .await?
            .lines()
            .map(String::from)
            .collect())
    }

    /// Stash including untracked files. (Applying/popping is left to the
    /// user or explicit approval; dropping is not exposed.)
    pub async fn stash_push(&self, message: &str) -> Result<(), GitError> {
        self.ok(&["stash", "push", "--include-untracked", "-m", message])
            .await
            .map(|_| ())
    }

    pub async fn worktrees(&self) -> Result<Vec<Worktree>, GitError> {
        let out = self.ok(&["worktree", "list", "--porcelain"]).await?;
        let mut v = Vec::new();
        let mut cur: Option<Worktree> = None;
        for line in out.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                if let Some(w) = cur.take() {
                    v.push(w);
                }
                cur = Some(Worktree {
                    path: p.into(),
                    head: None,
                    branch: None,
                });
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                if let Some(w) = cur.as_mut() {
                    w.head = Some(h.into());
                }
            } else if let Some(b) = line.strip_prefix("branch ") {
                if let Some(w) = cur.as_mut() {
                    w.branch = Some(b.trim_start_matches("refs/heads/").into());
                }
            }
        }
        if let Some(w) = cur {
            v.push(w);
        }
        Ok(v)
    }

    /// Isolated worktree on a new branch for parallel agent work.
    pub async fn add_worktree(&self, path: &Path, branch: &str) -> Result<(), GitError> {
        validate_ref(branch)?;
        let p = path.to_string_lossy().to_string();
        self.ok(&["worktree", "add", "-b", branch, &p])
            .await
            .map(|_| ())
    }

    /// Remove a worktree only if it has no uncommitted changes.
    pub async fn remove_worktree(&self, path: &Path) -> Result<(), GitError> {
        let wt = Git::new(path);
        let st = wt.status().await?;
        if !st.is_clean() {
            return Err(GitError::Refused(format!(
                "worktree {} has uncommitted changes",
                path.display()
            )));
        }
        let p = path.to_string_lossy().to_string();
        self.ok(&["worktree", "remove", &p]).await.map(|_| ())
    }

    pub async fn blame(&self, file: &str, start: usize, end: usize) -> Result<String, GitError> {
        let range = format!("{start},{end}");
        self.ok(&["blame", "-L", &range, "--date=short", "--", file])
            .await
    }

    /// Merge a branch into the current one. Refuses with a dirty tree so user
    /// changes are never mixed into a merge. Conflicts are reported, not
    /// auto-resolved.
    pub async fn merge(&self, branch: &str) -> Result<MergeOutcome, GitError> {
        validate_ref(branch)?;
        let st = self.status().await?;
        if !st.is_clean() {
            return Err(GitError::Refused(
                "working tree has uncommitted changes; commit or stash first".into(),
            ));
        }
        let r = self.run(&["merge", "--no-ff", "--no-edit", branch]).await?;
        if r.success() {
            if r.stdout.contains("Already up to date") {
                return Ok(MergeOutcome::UpToDate);
            }
            return Ok(MergeOutcome::Merged);
        }
        let conflicts = self.conflicts().await?;
        if conflicts.is_empty() {
            return Err(GitError::Failed {
                cmd: "merge".into(),
                stderr: r.stderr,
            });
        }
        Ok(MergeOutcome::Conflicts(conflicts))
    }

    /// Abort an in-progress merge (restores pre-merge state; safe because we
    /// only merge from a clean tree).
    pub async fn merge_abort(&self) -> Result<(), GitError> {
        self.ok(&["merge", "--abort"]).await.map(|_| ())
    }

    pub async fn conflicts(&self) -> Result<Vec<String>, GitError> {
        let out = self.ok(&["diff", "--name-only", "--diff-filter=U"]).await?;
        Ok(out
            .lines()
            .map(String::from)
            .filter(|l| !l.is_empty())
            .collect())
    }

    /// Both sides of a conflicted file: (ours, theirs).
    pub async fn conflict_versions(&self, file: &str) -> Result<(String, String), GitError> {
        let ours = self.ok(&["show", &format!(":2:{file}")]).await?;
        let theirs = self.ok(&["show", &format!(":3:{file}")]).await?;
        Ok((ours, theirs))
    }

    pub(crate) async fn ok_with_env(
        &self,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<String, GitError> {
        self.ok_env(args, env).await
    }
}

fn validate_ref(name: &str) -> Result<(), GitError> {
    let bad = name.is_empty()
        || name.starts_with('-')
        || name.contains("..")
        || name.contains(' ')
        || name.contains('~')
        || name.contains('^')
        || name.contains(':')
        || name.contains('\\')
        || name.ends_with(".lock")
        || name.ends_with('/');
    if bad {
        Err(GitError::Refused(format!("invalid branch name '{name}'")))
    } else {
        Ok(())
    }
}

pub fn parse_status_v2(out: &str) -> Status {
    let mut s = Status::default();
    let mut entries = out.split('\0').peekable();
    while let Some(e) = entries.next() {
        if e.is_empty() {
            continue;
        }
        if let Some(rest) = e.strip_prefix("# ") {
            if let Some(b) = rest.strip_prefix("branch.head ") {
                s.branch = (b != "(detached)").then(|| b.to_string());
            } else if let Some(u) = rest.strip_prefix("branch.upstream ") {
                s.upstream = Some(u.to_string());
            } else if let Some(ab) = rest.strip_prefix("branch.ab ") {
                for part in ab.split_whitespace() {
                    if let Some(a) = part.strip_prefix('+') {
                        s.ahead = a.parse().unwrap_or(0);
                    } else if let Some(b) = part.strip_prefix('-') {
                        s.behind = b.parse().unwrap_or(0);
                    }
                }
            } else if let Some(oid) = rest.strip_prefix("branch.oid ") {
                s.head = (oid != "(initial)").then(|| oid.to_string());
            }
            continue;
        }
        let kind = e.chars().next().unwrap_or(' ');
        match kind {
            '1' | '2' => {
                // 1 XY sub mH mI mW hH hI path
                // 2 XY sub mH mI mW hH hI Xscore path\0origPath
                let fields: Vec<&str> = e.splitn(if kind == '1' { 9 } else { 10 }, ' ').collect();
                let xy = fields.get(1).copied().unwrap_or("..");
                let path = fields.last().copied().unwrap_or("").to_string();
                if kind == '2' {
                    entries.next(); // original path
                }
                let mut chars = xy.chars();
                let x = chars.next().unwrap_or('.');
                let y = chars.next().unwrap_or('.');
                if x != '.' {
                    s.staged.push(path.clone());
                }
                if y != '.' {
                    s.modified.push(path);
                }
            }
            'u' => {
                let path = e.splitn(11, ' ').last().unwrap_or("").to_string();
                s.conflicted.push(path);
            }
            '?' => s.untracked.push(e[2..].to_string()),
            _ => {}
        }
    }
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub async fn repo() -> (tempfile::TempDir, Git) {
        let d = tempfile::tempdir().unwrap();
        let g = Git::new(d.path());
        g.init().await.unwrap();
        g.ok(&["config", "user.email", "test@example.com"])
            .await
            .unwrap();
        g.ok(&["config", "user.name", "Test"]).await.unwrap();
        g.ok(&["config", "commit.gpgsign", "false"]).await.unwrap();
        std::fs::write(d.path().join("a.txt"), "one\n").unwrap();
        g.commit("init", &["a.txt"]).await.unwrap();
        (d, g)
    }

    #[test]
    fn parses_porcelain_v2() {
        let out = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\u{0}1 .M N... 100644 100644 100644 a b src/x.rs\u{0}1 A. N... 000000 100644 100644 a b new.rs\0? untracked file.txt\0u UU N... 1 2 3 4 h1 h2 h3 conflict.rs\0";
        let s = parse_status_v2(out);
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert_eq!(s.modified, vec!["src/x.rs"]);
        assert_eq!(s.staged, vec!["new.rs"]);
        assert_eq!(s.untracked, vec!["untracked file.txt"]);
        assert_eq!(s.conflicted, vec!["conflict.rs"]);
    }

    #[tokio::test]
    async fn status_commit_log_branches() {
        let (d, g) = repo().await;
        assert!(g.is_repo().await);
        let st = g.status().await.unwrap();
        assert!(st.is_clean(), "{st:?}");
        std::fs::write(d.path().join("a.txt"), "two\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "new\n").unwrap();
        let st = g.status().await.unwrap();
        assert_eq!(st.modified, vec!["a.txt"]);
        assert_eq!(st.untracked, vec!["b.txt"]);
        assert!(g.diff(false, &[]).await.unwrap().contains("+two"));
        g.commit("second", &["a.txt", "b.txt"]).await.unwrap();
        assert_eq!(g.log(5).await.unwrap()[0].subject, "second");
        g.create_branch("feature/x", true).await.unwrap();
        assert_eq!(
            g.current_branch().await.unwrap().as_deref(),
            Some("feature/x")
        );
        assert!(g.branches().await.unwrap().len() >= 2);
        assert!(g.create_branch("-evil", false).await.is_err());
        assert!(g.commit("x", &[]).await.is_err());
        assert!(g.blame("a.txt", 1, 1).await.unwrap().contains("two"));
    }

    #[tokio::test]
    async fn merge_reports_conflicts_and_refuses_dirty() {
        let (d, g) = repo().await;
        let main = g.current_branch().await.unwrap().unwrap();
        g.create_branch("other", true).await.unwrap();
        std::fs::write(d.path().join("a.txt"), "theirs\n").unwrap();
        g.commit("theirs", &["a.txt"]).await.unwrap();
        g.ok(&["switch", &main]).await.unwrap();
        std::fs::write(d.path().join("a.txt"), "ours\n").unwrap();
        // dirty tree -> refuse
        assert!(matches!(g.merge("other").await, Err(GitError::Refused(_))));
        g.commit("ours", &["a.txt"]).await.unwrap();
        match g.merge("other").await.unwrap() {
            MergeOutcome::Conflicts(files) => {
                assert_eq!(files, vec!["a.txt"]);
                let (o, t) = g.conflict_versions("a.txt").await.unwrap();
                assert_eq!((o.trim(), t.trim()), ("ours", "theirs"));
            }
            other => panic!("expected conflicts, got {other:?}"),
        }
        g.merge_abort().await.unwrap();
        assert!(g.status().await.unwrap().is_clean());
    }

    #[tokio::test]
    async fn worktrees_isolate_and_protect_changes() {
        let (d, g) = repo().await;
        let wt_path = d.path().join("wt-agent");
        g.add_worktree(&wt_path, "agent/task-1").await.unwrap();
        let wts = g.worktrees().await.unwrap();
        assert!(wts
            .iter()
            .any(|w| w.branch.as_deref() == Some("agent/task-1")));
        std::fs::write(wt_path.join("work.txt"), "wip").unwrap();
        assert!(matches!(
            g.remove_worktree(&wt_path).await,
            Err(GitError::Refused(_))
        ));
        std::fs::remove_file(wt_path.join("work.txt")).unwrap();
        g.remove_worktree(&wt_path).await.unwrap();
    }
}
