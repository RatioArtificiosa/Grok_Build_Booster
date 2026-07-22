use anyhow::{Context, Result};
use git2::Repository;
use std::path::Path;

/// Create a non-destructive recovery branch at HEAD. Does not touch the working tree.
pub fn create_recovery_branch(repo_path: &Path, label: &str) -> Result<String> {
    let repo = Repository::discover(repo_path)
        .or_else(|_| Repository::open(repo_path))
        .context("open git repo")?;
    let head = repo
        .head()
        .context("no HEAD — make an initial commit first")?
        .peel_to_commit()
        .context("peel HEAD to commit")?;
    let oid = head.id();
    let oid_str = oid.to_string();
    let short = &oid_str[..7.min(oid_str.len())];

    let safe: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(40)
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let safe = if safe.is_empty() {
        "bookmark".into()
    } else {
        safe
    };

    let branch_name = format!(
        "_grok_booster_recovery/{}-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
        safe
    );

    // force=false: if the exact name exists (same second), append pid
    if repo.branch(&branch_name, &head, false).is_ok() {
        return Ok(format!("{branch_name} @ {short}"));
    }
    let branch_name = format!("{branch_name}-{}", std::process::id());
    repo.branch(&branch_name, &head, false)
        .context("create recovery branch")?;
    Ok(format!("{branch_name} @ {short}"))
}

/// Record current HEAD oid (no auto-commit). None if not a git repo.
pub fn try_snapshot_branch(repo_path: &str) -> Result<Option<String>> {
    let path = Path::new(repo_path);
    let repo = match Repository::discover(path).or_else(|_| Repository::open(path)) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    if let Ok(head) = repo.head() {
        if let Ok(commit) = head.peel_to_commit() {
            return Ok(Some(commit.id().to_string()));
        }
    }
    Ok(None)
}

/// List paths that differ from HEAD (for rewind dry-run preview).
pub fn dirty_files(repo_path: &Path) -> Result<Vec<String>> {
    let repo = Repository::discover(repo_path).or_else(|_| Repository::open(repo_path))?;
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    let mut out = Vec::new();
    for entry in statuses.iter() {
        if let Some(path) = entry.path() {
            out.push(path.to_string());
            if out.len() >= 500 {
                out.push("…(truncated)".into());
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn recovery_branch_on_clean_repo() {
        let dir = tempdir().unwrap();
        let path = dir.path();
        assert!(Command::new("git")
            .args(["init"])
            .current_dir(path)
            .status()
            .unwrap()
            .success());
        let _ = Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(path)
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(path)
            .status();
        std::fs::write(path.join("a.txt"), "hi").unwrap();
        assert!(Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(path)
            .status()
            .unwrap()
            .success());

        let ref_name = create_recovery_branch(path, "test prompt!").unwrap();
        assert!(ref_name.contains("_grok_booster_recovery"));
    }
}
