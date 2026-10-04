//! Checkpoints: lightweight, restorable snapshots of the working tree.
//!
//! Implementation uses Git objects through a *temporary index*, so creating a
//! checkpoint never touches the user's index, branches, HEAD or stash. Each
//! checkpoint is a commit referenced by `refs/conductor/checkpoints/<id>`;
//! Git deduplicates content, so checkpoints of large repositories are cheap.
//!
//! Restoring first records a "before restore" checkpoint, so a restore can
//! itself be undone.

use serde::{Deserialize, Serialize};

use crate::git::{Git, GitError};

const REF_PREFIX: &str = "refs/conductor/checkpoints/";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub commit: String,
    pub label: String,
    pub created: String,
}

pub struct Checkpoints<'a> {
    git: &'a Git,
}

impl<'a> Checkpoints<'a> {
    pub fn new(git: &'a Git) -> Self {
        Self { git }
    }

    fn tmp_index(&self) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("conductor-index-{}", uuid::Uuid::new_v4().simple()))
    }

    /// Snapshot the full working tree (tracked + untracked, respecting
    /// .gitignore) into a tree object; returns (tree, parent_commit).
    async fn snapshot_tree(&self) -> Result<(String, Option<String>), GitError> {
        let idx = self.tmp_index();
        let idx_s = idx.to_string_lossy().to_string();
        let env = [("GIT_INDEX_FILE", idx_s.as_str())];
        let head = self
            .git
            .ok_with_env(&["rev-parse", "--verify", "-q", "HEAD"], &[])
            .await
            .ok()
            .map(|s| s.trim().to_string());
        if head.is_some() {
            self.git.ok_with_env(&["read-tree", "HEAD"], &env).await?;
        }
        let result = async {
            self.git
                .ok_with_env(&["add", "-A", "--", "."], &env)
                .await?;
            let tree = self.git.ok_with_env(&["write-tree"], &env).await?;
            Ok::<_, GitError>(tree.trim().to_string())
        }
        .await;
        let _ = std::fs::remove_file(&idx);
        Ok((result?, head))
    }

    pub async fn create(&self, label: &str) -> Result<Checkpoint, GitError> {
        let (tree, parent) = self.snapshot_tree().await?;
        let msg = format!("conductor checkpoint: {label}");
        let mut args = vec!["commit-tree", tree.as_str(), "-m", msg.as_str()];
        if let Some(p) = &parent {
            args.push("-p");
            args.push(p.as_str());
        }
        let env = [
            ("GIT_AUTHOR_NAME", "Conductor"),
            ("GIT_AUTHOR_EMAIL", "checkpoints@conductor.local"),
            ("GIT_COMMITTER_NAME", "Conductor"),
            ("GIT_COMMITTER_EMAIL", "checkpoints@conductor.local"),
        ];
        let commit = self.git.ok_with_env(&args, &env).await?.trim().to_string();
        let id = format!(
            "{}-{}",
            crate::unix_now(),
            &uuid::Uuid::new_v4().simple().to_string()[..6]
        );
        let refname = format!("{REF_PREFIX}{id}");
        self.git
            .ok_with_env(&["update-ref", &refname, &commit], &[])
            .await?;
        Ok(Checkpoint {
            id,
            commit,
            label: label.to_string(),
            created: String::new(),
        })
    }

    pub async fn list(&self) -> Result<Vec<Checkpoint>, GitError> {
        let out = self
            .git
            .ok_with_env(
                &["for-each-ref", "--sort=-creatordate", "--format=%(refname)%1f%(objectname)%1f%(contents:subject)%1f%(creatordate:iso-strict)", REF_PREFIX],
                &[],
            )
            .await?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let p: Vec<&str> = l.split('\x1f').collect();
                (p.len() == 4).then(|| Checkpoint {
                    id: p[0].trim_start_matches(REF_PREFIX).to_string(),
                    commit: p[1].to_string(),
                    label: p[2]
                        .trim_start_matches("conductor checkpoint: ")
                        .to_string(),
                    created: p[3].to_string(),
                })
            })
            .collect())
    }

    /// Files changed between a checkpoint and the current working tree.
    pub async fn changed_since(&self, id: &str) -> Result<Vec<String>, GitError> {
        let cp = self.get(id).await?;
        let (tree, _) = self.snapshot_tree().await?;
        let out = self
            .git
            .ok_with_env(&["diff", "--name-only", &cp.commit, &tree], &[])
            .await?;
        Ok(out
            .lines()
            .map(String::from)
            .filter(|l| !l.is_empty())
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Checkpoint, GitError> {
        self.list()
            .await?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| GitError::Refused(format!("checkpoint {id} not found")))
    }

    /// Restore the working tree to a checkpoint. Creates a safety checkpoint
    /// first (returned) so nothing is lost. Does not modify the user's index
    /// or HEAD.
    pub async fn restore(&self, id: &str) -> Result<Checkpoint, GitError> {
        let target = self.get(id).await?;
        let safety = self
            .create(&format!("before restoring {}", target.label))
            .await?;
        // Files that exist now but not in the checkpoint get removed (they
        // remain recoverable from the safety checkpoint).
        let added = self
            .git
            .ok_with_env(
                &[
                    "diff",
                    "--name-only",
                    "--diff-filter=A",
                    &target.commit,
                    &safety.commit,
                ],
                &[],
            )
            .await?;
        for f in added.lines().filter(|l| !l.is_empty()) {
            let p = self.git.root().join(f);
            let _ = std::fs::remove_file(p);
        }
        let idx = self.tmp_index();
        let idx_s = idx.to_string_lossy().to_string();
        let env = [("GIT_INDEX_FILE", idx_s.as_str())];
        let r = async {
            self.git
                .ok_with_env(&["read-tree", &target.commit], &env)
                .await?;
            self.git
                .ok_with_env(&["checkout-index", "-a", "-f"], &env)
                .await?;
            Ok::<_, GitError>(())
        }
        .await;
        let _ = std::fs::remove_file(&idx);
        r?;
        Ok(safety)
    }

    /// Delete a checkpoint reference (content is garbage-collected by Git).
    pub async fn delete(&self, id: &str) -> Result<(), GitError> {
        let refname = format!("{REF_PREFIX}{id}");
        self.git
            .ok_with_env(&["update-ref", "-d", &refname], &[])
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::repo;

    #[tokio::test]
    async fn create_and_restore_roundtrip_without_touching_index() {
        let (d, g) = repo().await;
        std::fs::write(d.path().join("a.txt"), "staged change\n").unwrap();
        g.run(&["add", "a.txt"]).await.unwrap();
        std::fs::write(d.path().join("untracked.txt"), "keep me\n").unwrap();
        let before_status = g.status().await.unwrap();

        let cps = Checkpoints::new(&g);
        let cp = cps.create("before refactor").await.unwrap();
        // user's index and status unchanged by checkpointing
        assert_eq!(g.status().await.unwrap(), before_status);

        // Make destructive changes.
        std::fs::write(d.path().join("a.txt"), "broken\n").unwrap();
        std::fs::remove_file(d.path().join("untracked.txt")).unwrap();
        std::fs::write(d.path().join("new_junk.txt"), "junk\n").unwrap();
        assert_eq!(cps.changed_since(&cp.id).await.unwrap().len(), 3);

        let safety = cps.restore(&cp.id).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(d.path().join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "staged change\n"
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join("untracked.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "keep me\n"
        );
        assert!(!d.path().join("new_junk.txt").exists());

        // The restore itself can be undone.
        cps.restore(&safety.id).await.unwrap();
        assert!(d.path().join("new_junk.txt").exists());

        let list = cps.list().await.unwrap();
        assert!(list.iter().any(|c| c.label == "before refactor"));
        cps.delete(&cp.id).await.unwrap();
        assert!(!cps.list().await.unwrap().iter().any(|c| c.id == cp.id));
        // HEAD untouched: still one user commit
        assert_eq!(g.log(10).await.unwrap().len(), 1);
    }
}
