//! Bounded ref discovery. Does not fetch, inspect object contents, or change checkout state.
use diffkin_domain::{RepositoryError, RepositoryRef, RepositoryRefKind, RepositoryRefs};
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
const REF_LIMIT: usize = 4096;
const OUTPUT_LIMIT: usize = 1024 * 1024;
fn invalid(message: impl ToString) -> RepositoryError {
    RepositoryError::IoFailure {
        message: message.to_string(),
    }
}
fn check(cancel: &AtomicBool) -> Result<(), RepositoryError> {
    if cancel.load(Ordering::Relaxed) {
        Err(RepositoryError::Cancelled {
            message: "Ref discovery cancelled".into(),
        })
    } else {
        Ok(())
    }
}
fn command(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(root)
        .args([
            "--no-pager",
            "--no-replace-objects",
            "--no-lazy-fetch",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "core.pager=cat",
            "-c",
            "protocol.allow=never",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("LC_ALL", "C");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_NAMESPACE",
        "GIT_REPLACE_REF_BASE",
    ] {
        c.env_remove(key);
    }
    c
}
fn output(
    root: &Path,
    args: &[String],
    cancel: &AtomicBool,
) -> Result<(bool, Vec<u8>), RepositoryError> {
    check(cancel)?;
    let mut child = command(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| RepositoryError::GitUnavailable {
            message: error.to_string(),
        })?;
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take((OUTPUT_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = send.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut completed_output = None;
    let result = loop {
        if let Err(error) = check(cancel) {
            break Err(error);
        }
        if Instant::now() >= deadline {
            break Err(RepositoryError::BudgetExceeded {
                message: "Ref discovery exceeded its five-second budget".into(),
            });
        }
        if let Some(bytes) = completed_output.take() {
            match child.try_wait() {
                Ok(Some(status)) => break Ok((status.success(), bytes)),
                Ok(None) => {
                    completed_output = Some(bytes);
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => break Err(invalid(error)),
            }
        }
        match receive.recv_timeout(Duration::from_millis(10)) {
            Ok(Ok(bytes)) if bytes.len() > OUTPUT_LIMIT => {
                break Err(RepositoryError::BudgetExceeded {
                    message: "Ref discovery exceeded its 1 MiB output budget; narrow the filter"
                        .into(),
                })
            }
            Ok(Ok(bytes)) => {
                completed_output = Some(bytes);
            }
            Ok(Err(error)) => break Err(invalid(error)),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(invalid("Ref reader stopped unexpectedly"))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = reader.join();
    result
}
/// query is a literal case-insensitive substring; pagination is bounded to 4096 matches.
/// Results reflect one live ref listing, not a frozen comparison. Commit input remains supported.
pub fn list_refs(
    repository: &str,
    query: &str,
    offset: u32,
    limit: u32,
    cancel: &AtomicBool,
) -> Result<RepositoryRefs, RepositoryError> {
    check(cancel)?;
    if repository.len() > 4096
        || query.len() > 128
        || query
            .chars()
            .any(|c| c.is_control() || matches!(c, '*' | '?' | '[' | ']' | '\\'))
        || offset as usize > REF_LIMIT
        || !(1..=200).contains(&limit)
    {
        return Err(invalid(
            "Invalid repository, literal ref filter or page (limit 1–200, offset ≤4096)",
        ));
    }
    let root =
        fs::canonicalize(repository).map_err(|error| RepositoryError::InvalidRepository {
            message: error.to_string(),
        })?;
    if !root.is_dir() {
        return Err(RepositoryError::InvalidRepository {
            message: "Repository path is not a directory".into(),
        });
    }
    let mut args = vec![
        "for-each-ref".into(),
        "--sort=refname".into(),
        "--ignore-case".into(),
        format!("--count={}", REF_LIMIT + 1),
        "--format=%(refname)%00%(objectname)%00%(symref)".into(),
        "--".into(),
    ];
    for prefix in ["refs/heads/", "refs/remotes/", "refs/tags/"] {
        if query.is_empty() {
            args.push(prefix.into());
        } else {
            // Git wildmatch treats slash as a separator. Recursive prefix/suffix
            // patterns keep a literal substring discoverable at every ref depth.
            args.push(format!("{prefix}**/*{query}*"));
            args.push(format!("{prefix}**/*{query}*/**"));
            // A pasted full ref (or a substring crossing its namespace) must
            // match too. Only add patterns within the three allowed namespaces,
            // so unrelated refs cannot consume the bounded match budget.
            let folded_query = query.to_ascii_lowercase();
            for start in 0..prefix.len() {
                let suffix = &prefix[start..]; // namespace prefixes are ASCII
                if suffix.starts_with(&folded_query) {
                    args.push(prefix.into());
                    break;
                }
                if folded_query.starts_with(suffix) {
                    let tail = &query[suffix.len()..];
                    args.push(format!("{prefix}{tail}*"));
                    args.push(format!("{prefix}{tail}*/**"));
                }
            }
        }
    }
    let (success, bytes) = output(&root, &args, cancel)?;
    if !success {
        return Err(RepositoryError::InvalidRepository {
            message: "Git could not list local refs in this directory; no fetch was attempted"
                .into(),
        });
    }
    let record_count = bytes
        .split(|b| *b == b'\n')
        .filter(|r| !r.is_empty())
        .count();
    let mut refs = Vec::new();
    let mut warnings = Vec::new();
    for record in bytes.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
        let record = record.strip_suffix(b"\r").unwrap_or(record);
        let parts: Vec<_> = record.split(|b| *b == 0).collect();
        if parts.len() != 3 {
            return Err(invalid("Malformed Git ref record"));
        }
        let name = match std::str::from_utf8(parts[0]) {
            Ok(name) => name,
            Err(_) => {
                warnings.push(
                    "A non-UTF8 ref cannot be displayed; use a valid UTF8 ref or commit ID".into(),
                );
                continue;
            }
        };
        let Some((prefix, kind)) = [
            ("refs/heads/", RepositoryRefKind::Branch),
            ("refs/remotes/", RepositoryRefKind::Remote),
            ("refs/tags/", RepositoryRefKind::Tag),
        ]
        .into_iter()
        .find(|(prefix, _)| name.starts_with(prefix)) else {
            continue;
        };
        refs.push(RepositoryRef {
            name: name.into(),
            label: name[prefix.len()..].into(),
            kind,
            target: String::from_utf8_lossy(parts[1]).into(),
            symbolic_target: (!parts[2].is_empty())
                .then(|| String::from_utf8_lossy(parts[2]).into()),
        });
    }
    let truncated = record_count > REF_LIMIT;
    refs.sort_by(|a, b| a.name.cmp(&b.name));
    refs.truncate(REF_LIMIT);
    if truncated {
        warnings.push("Ref list is limited to 4096 matches. Narrow the filter or enter a branch, tag or commit directly; remote refs are local cached refs and were not fetched.".into());
    }
    warnings.sort();
    warnings.dedup();
    let count = refs.len();
    let start = (offset as usize).min(count);
    let end = (start + limit as usize).min(count);
    let next_offset = (end < count).then_some(end as u32);
    let (head_ok, head) = output(
        &root,
        &["symbolic-ref".into(), "-q".into(), "HEAD".into()],
        cancel,
    )?;
    let current_branch = head_ok.then(|| String::from_utf8_lossy(&head).trim().to_string());
    Ok(RepositoryRefs {
        repository: root.to_string_lossy().into(),
        refs: refs.drain(start..end).collect(),
        current_branch,
        offset,
        next_offset,
        truncated,
        warnings,
    })
}
