//! Read-only Git snapshots. No diff, filters, hooks, checkout or lazy object fetching.
use diffkin_domain::{
    CompareMode, ComparisonManifest, ComparisonSpec, FileStatus, FileSummary, FrozenFile,
    RepositoryError,
};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata},
    io::{BufRead, BufReader, Read, Write},
    path::{Component, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant, SystemTime},
};

const MAX_FILE: usize = 2 * 1024 * 1024;
const MAX_SNAPSHOT: usize = 64 * 1024 * 1024;
const MAX_GIT_OUTPUT: usize = 80 * 1024 * 1024;

const MAX_CHANGES: usize = 2_000;
const CAPTURE_BUDGET: Duration = Duration::from_secs(20);

fn io_error(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::IoFailure {
        message: error.to_string(),
    }
}
fn budget(message: &str) -> RepositoryError {
    RepositoryError::BudgetExceeded {
        message: message.into(),
    }
}
fn stale(message: &str) -> RepositoryError {
    RepositoryError::Stale {
        message: message.into(),
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct Context<'a> {
    root: PathBuf,
    cancel: &'a AtomicBool,
    deadline: Instant,
}
struct Output<T = Vec<u8>> {
    success: bool,
    stdout: T,
    stderr: String,
}

impl Context<'_> {
    fn check(&self) -> Result<(), RepositoryError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(RepositoryError::Cancelled {
                message: "Comparison cancelled".into(),
            });
        }
        if Instant::now() >= self.deadline {
            return Err(budget("Snapshot exceeded its 20 second budget"));
        }
        Ok(())
    }

    fn git_read<T: Send>(
        &self,
        arguments: &[&str],
        input: Option<&[u8]>,
        read: impl FnOnce(std::process::ChildStdout) -> Result<T, RepositoryError> + Send,
    ) -> Result<Output<T>, RepositoryError> {
        self.check()?;
        let mut command = Command::new("git");
        command
            .current_dir(&self.root)
            .args([
                "--no-pager",
                "--no-replace-objects",
                "--no-lazy-fetch",
                "--literal-pathspecs",
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
            .args(arguments)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in [
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
            command.env_remove(name);
        }
        let mut child = command
            .spawn()
            .map_err(|error| RepositoryError::GitUnavailable {
                message: format!("Unable to start system Git: {error}"),
            })?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let stdin = child.stdin.take().expect("piped stdin");
        thread::scope(|scope| {
            let out = scope.spawn(move || read(stdout));
            let err = scope.spawn(move || read_bounded(stderr, 256 * 1024));
            let write = scope.spawn(move || {
                let mut stdin = stdin;
                if let Some(data) = input {
                    stdin.write_all(data)
                } else {
                    Ok(())
                }
            });
            let status = loop {
                if let Err(error) = self.check() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => thread::sleep(Duration::from_millis(2)),
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(io_error(error));
                    }
                }
            };
            let stdout = out
                .join()
                .map_err(|_| io_error("Git stdout reader panicked"))??;
            let stderr = err
                .join()
                .map_err(|_| io_error("Git stderr reader panicked"))??;
            let _ = write
                .join()
                .map_err(|_| io_error("Git input writer panicked"))?;
            Ok(Output {
                success: status.success(),
                stdout,
                stderr: String::from_utf8_lossy(&stderr).trim().into(),
            })
        })
    }

    fn git(&self, arguments: &[&str], input: Option<&[u8]>) -> Result<Output, RepositoryError> {
        self.git_read(arguments, input, |stdout| {
            read_bounded(stdout, MAX_GIT_OUTPUT)
        })
    }

    fn required(&self, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, RepositoryError> {
        let output = self.git(args, input)?;
        if !output.success {
            return Err(io_error(format!(
                "Git {} failed: {}",
                args[0], output.stderr
            )));
        }
        Ok(output.stdout)
    }

    fn resolve(&self, revision: &str) -> Result<String, RepositoryError> {
        if revision.is_empty() || revision.len() > 1024 || revision.contains('\0') {
            return Err(RepositoryError::InvalidRevision {
                message: "Invalid revision".into(),
            });
        }
        let query = format!("{revision}^{{commit}}");
        let output = self.git(&["rev-parse", "--verify", "--end-of-options", &query], None)?;
        if !output.success {
            return Err(RepositoryError::InvalidRevision {
                message: format!("Cannot resolve {revision}: {}", output.stderr),
            });
        }
        let oid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        validate_oid(&oid)?;
        Ok(oid)
    }

    fn head(&self) -> Result<Option<String>, RepositoryError> {
        match self.resolve("HEAD") {
            Ok(oid) => Ok(Some(oid)),
            Err(RepositoryError::InvalidRevision { .. }) => {
                let symbolic = self.git(&["symbolic-ref", "--quiet", "HEAD"], None)?;
                if !symbolic.success {
                    return Err(RepositoryError::InvalidRevision {
                        message: "HEAD is invalid".into(),
                    });
                }
                let reference = String::from_utf8_lossy(&symbolic.stdout).trim().to_owned();
                let exists = self.git(&["show-ref", "--verify", "--quiet", &reference], None)?;
                if exists.success {
                    return Err(RepositoryError::InvalidRevision {
                        message: "HEAD references an unavailable commit".into(),
                    });
                }
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

fn read_bounded(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, RepositoryError> {
    let mut data = Vec::new();
    reader
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut data)
        .map_err(io_error)?;
    if data.len() > limit {
        return Err(budget("Git output exceeded the snapshot limit"));
    }
    Ok(data)
}

fn validate_oid(oid: &str) -> Result<(), RepositoryError> {
    if !matches!(oid.len(), 40 | 64)
        || !oid.bytes().all(|b| b.is_ascii_hexdigit())
        || oid.bytes().all(|b| b == b'0')
    {
        return Err(io_error("Invalid Git object ID"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    mode: String,
    oid: String,
    conflict: bool,
}
type Entries = BTreeMap<Vec<u8>, Entry>;

/// Raw tree/index diff metadata is bounded by actual changes, not repository size.
fn raw_diff(
    context: &Context<'_>,
    base: Option<&str>,
    head: Option<&str>,
) -> Result<(Entries, Entries, BTreeSet<Vec<u8>>), RepositoryError> {
    let args = if let Some(head) = head {
        vec![
            "diff-tree",
            "-r",
            "--raw",
            "--no-abbrev",
            "-z",
            "--no-renames",
            "--ignore-submodules=none",
            "--no-ext-diff",
            "--no-textconv",
            base.expect("commit base"),
            head,
            "--",
        ]
    } else {
        let mut args = vec![
            "diff",
            "--cached",
            "--raw",
            "--no-abbrev",
            "-z",
            "--no-renames",
            "--ignore-submodules=none",
            "--no-ext-diff",
            "--no-textconv",
            "--ita-invisible-in-index",
        ];
        if let Some(base) = base {
            args.push(base);
        }
        args.push("--");
        args
    };
    // Explicit raw format never invokes an external diff or text conversion.
    let bytes = context.required(&args, None)?;
    let mut before = Entries::new();
    let mut after = Entries::new();
    let mut paths = BTreeSet::new();
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    for header in records.by_ref().collect::<Vec<_>>().chunks(2) {
        if header.len() != 2 {
            return Err(io_error("Malformed raw Git diff"));
        }
        let fields = std::str::from_utf8(header[0])
            .map_err(io_error)?
            .strip_prefix(':')
            .ok_or_else(|| io_error("Malformed raw Git entry"))?
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 5 || fields[4].len() != 1 {
            return Err(io_error("Unexpected raw Git diff layout"));
        }
        let path = header[1].to_vec();
        validate_path(&path)?;
        paths.insert(path.clone());
        for (mode, oid, target) in [
            (fields[0], fields[2], &mut before),
            (fields[1], fields[3], &mut after),
        ] {
            if mode == "000000" || oid.bytes().all(|byte| byte == b'0') {
                continue;
            }
            if !matches!(mode, "100644" | "100755" | "120000" | "160000") {
                return Err(io_error("Unsupported raw Git mode"));
            }
            validate_oid(oid)?;
            target.insert(
                path.clone(),
                Entry {
                    mode: mode.into(),
                    oid: oid.into(),
                    conflict: fields[4] == "U",
                },
            );
        }
    }
    Ok((before, after, paths))
}

// Portable Git debug fields are used for the conservative Unix stat fast path.
#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Default, Clone)]
struct IndexStat {
    ctime: (i64, i64),
    mtime: (i64, i64),
    dev: u64,
    ino: u64,
    size: u64,
    flags: u32,
}
struct IndexRecord {
    path: Vec<u8>,
    entry: Entry,
    stat: IndexStat,
}
fn pair(line: &str, label: &str) -> Result<(i64, i64), RepositoryError> {
    let value = line
        .strip_prefix(label)
        .ok_or_else(|| io_error("Unsupported Git index metadata format"))?
        .trim();
    let (a, b) = value
        .split_once(':')
        .ok_or_else(|| io_error("Unsupported Git index timestamp"))?;
    Ok((
        a.trim().parse().map_err(io_error)?,
        b.trim().parse().map_err(io_error)?,
    ))
}
/// Parse one record at a time. A huge index never becomes a huge captured snapshot.
fn scan_index(
    context: &Context<'_>,
    mut visit: impl FnMut(IndexRecord) -> Result<(), RepositoryError> + Send,
) -> Result<(), RepositoryError> {
    let result = context.git_read(&["ls-files", "--stage", "--debug", "-z"], None, |stdout| {
        let mut reader = BufReader::new(stdout);
        let mut pending: Option<IndexRecord> = None;
        loop {
            context.check()?;
            let mut record = Vec::new();
            if reader
                .by_ref()
                .take(64 * 1024 + 1)
                .read_until(0, &mut record)
                .map_err(io_error)?
                == 0
            {
                break;
            }
            if record.len() > 64 * 1024 || record.pop() != Some(0) {
                return Err(io_error("Index path exceeds the bounded record size"));
            }
            let parsed = parse_entries(&record, true)?;
            let (path, entry) = parsed
                .into_iter()
                .next()
                .ok_or_else(|| io_error("Empty index entry"))?;
            let mut lines = Vec::new();
            for _ in 0..5 {
                let mut line = String::new();
                reader
                    .by_ref()
                    .take(1025)
                    .read_line(&mut line)
                    .map_err(io_error)?;
                if line.len() > 1024 || !line.ends_with('\n') {
                    return Err(io_error("Unsupported Git index debug format"));
                }
                lines.push(line);
            }
            let ctime = pair(&lines[0], "  ctime:")?;
            let mtime = pair(&lines[1], "  mtime:")?;
            let dev_line = lines[2]
                .strip_prefix("  dev:")
                .ok_or_else(|| io_error("Unsupported index device field"))?;
            let (dev, ino) = dev_line
                .split_once("ino:")
                .ok_or_else(|| io_error("Unsupported index inode field"))?;
            if !lines[3].starts_with("  uid:") {
                return Err(io_error("Unsupported index user field"));
            }
            let size_line = lines[4]
                .strip_prefix("  size:")
                .ok_or_else(|| io_error("Unsupported index size field"))?;
            let (size, flags) = size_line
                .split_once("flags:")
                .ok_or_else(|| io_error("Index flags are unavailable"))?;
            let stat = IndexStat {
                ctime,
                mtime,
                dev: dev.trim().parse().map_err(io_error)?,
                ino: ino.trim().parse().map_err(io_error)?,
                size: size.trim().parse().map_err(io_error)?,
                flags: u32::from_str_radix(flags.trim(), 16).map_err(io_error)?,
            };
            let next = IndexRecord { path, entry, stat };
            if let Some(previous) = &mut pending {
                if previous.path == next.path {
                    previous.entry.conflict |= next.entry.conflict;
                    previous.stat.flags |= next.stat.flags;
                    continue;
                }
                visit(pending.take().unwrap())?;
            }
            pending = Some(next);
        }
        if let Some(record) = pending {
            visit(record)?;
        }
        Ok(())
    })?;
    if !result.success {
        return Err(io_error(format!("Git ls-files failed: {}", result.stderr)));
    }
    Ok(())
}

fn index_modified(context: &Context<'_>) -> Result<Option<SystemTime>, RepositoryError> {
    let path = context.required(
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        None,
    )?;
    let path = String::from_utf8(path).map_err(io_error)?;
    match fs::metadata(path.trim_end()) {
        Ok(metadata) => Ok(metadata.modified().ok()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}
fn index_matches(
    record: &IndexRecord,
    metadata: &Metadata,
    index_time: Option<SystemTime>,
) -> bool {
    if record.entry.conflict
        || record.stat.flags & (0x2000_0000 | 0x4000_0000 | 0x8000) != 0
        || !metadata.is_file()
        || record.entry.mode == "120000"
        || record.entry.mode == "160000"
    {
        return false;
    }
    if index_time.is_none()
        || metadata
            .modified()
            .ok()
            .is_none_or(|time| Some(time) >= index_time)
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mode = if metadata.mode() & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        };
        record.entry.mode == mode
            && metadata.len() == record.stat.size
            && (metadata.ctime(), metadata.ctime_nsec()) == record.stat.ctime
            && (metadata.mtime(), metadata.mtime_nsec()) == record.stat.mtime
            && (metadata.dev() & 0xffff_ffff) == record.stat.dev
            && (metadata.ino() & 0xffff_ffff) == record.stat.ino
    }
    #[cfg(not(unix))]
    {
        false
    }
}
fn observe(
    context: &Context<'_>,
    path: &[u8],
    hash: &mut Sha256,
) -> Result<Option<Metadata>, RepositoryError> {
    context.check()?;
    let full = working_path(context, path)?;
    let meta = match fs::symlink_metadata(full) {
        Ok(meta) => Some(meta),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(error)),
    };
    hash.update((path.len() as u64).to_le_bytes());
    hash.update(path);
    hash.update(format!("{:?}", meta.as_ref().map(stamp)).as_bytes());
    Ok(meta)
}
fn index_observation(context: &Context<'_>) -> Result<String, RepositoryError> {
    let mut hash = Sha256::new();
    scan_index(context, |record| {
        observe(context, &record.path, &mut hash)?;
        Ok(())
    })?;
    Ok(format!("{:x}", hash.finalize()))
}

/// Fast-path only when no configuration/attribute can change the stored blob bytes.
struct Attributes {
    always: bool,
    global: bool,
    directories: BTreeMap<PathBuf, bool>,
    indexed_directories: BTreeSet<PathBuf>,
}
impl Attributes {
    fn new(context: &Context<'_>) -> Result<Self, RepositoryError> {
        // check-attr falls back to tracked attributes when a worktree attribute
        // file is missing. Keep only their parent directories, never the catalogue.
        let mut indexed_directories = BTreeSet::new();
        let mut attribute_bytes = 0;
        let mut attribute_overflow = false;
        let listing = context.git_read(&["ls-files", "--cached", "-z"], None, |stdout| {
            let mut reader = BufReader::new(stdout);
            loop {
                context.check()?;
                let mut path = Vec::new();
                if reader
                    .by_ref()
                    .take(64 * 1024 + 1)
                    .read_until(0, &mut path)
                    .map_err(io_error)?
                    == 0
                {
                    break;
                }
                if path.len() > 64 * 1024 || path.pop() != Some(0) {
                    return Err(io_error("Index path exceeds the bounded record size"));
                }
                if path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes".as_slice())
                    && !attribute_overflow
                {
                    validate_path(&path)?;
                    attribute_bytes += path.len();
                    if indexed_directories.len() >= 4096 || attribute_bytes > 1024 * 1024 {
                        indexed_directories.clear();
                        attribute_overflow = true;
                    } else {
                        let path = path_from_bytes(&path)?;
                        indexed_directories.insert(
                            path.parent()
                                .unwrap_or(std::path::Path::new(""))
                                .to_path_buf(),
                        );
                    }
                }
            }
            Ok(())
        })?;
        if !listing.success {
            return Err(io_error(format!(
                "Git attribute discovery failed: {}",
                listing.stderr
            )));
        }
        let autocrlf = context.git(&["config", "--get", "core.autocrlf"], None)?;
        let always = autocrlf.success
            && !matches!(
                String::from_utf8_lossy(&autocrlf.stdout).trim(),
                "false" | "0" | ""
            );
        let configured = context.git(&["config", "--get", "core.attributesFile"], None)?;
        // Git also loads default XDG/HOME and system attribute files without any
        // core.attributesFile setting. Ask Git for its paths; older Git versions
        // that cannot report them take the literal fallback instead of guessing.
        let mut global = configured.success;
        let mut unknown_attributes = false;
        for variable in ["GIT_ATTR_GLOBAL", "GIT_ATTR_SYSTEM"] {
            let output = context.git(&["var", variable], None)?;
            if !output.success {
                unknown_attributes = true;
                continue;
            }
            let path =
                path_from_bytes(output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout))?;
            if !path.as_os_str().is_empty() {
                match fs::symlink_metadata(path) {
                    Ok(_) => global = true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Err(_) => unknown_attributes = true,
                }
            }
        }
        let info = context.required(
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "info/attributes",
            ],
            None,
        )?;
        let info = String::from_utf8(info).map_err(io_error)?;
        Ok(Self {
            always: always || unknown_attributes || attribute_overflow,
            global: global || PathBuf::from(info.trim_end()).exists(),
            directories: BTreeMap::new(),
            indexed_directories,
        })
    }
    fn possible(&mut self, context: &Context<'_>, path: &[u8]) -> Result<bool, RepositoryError> {
        if self.always || self.global {
            return Ok(true);
        }
        let path = path_from_bytes(path)?;
        let mut directory = path
            .parent()
            .unwrap_or(std::path::Path::new(""))
            .to_path_buf();
        let mut visited = Vec::new();
        let result = loop {
            if let Some(value) = self.directories.get(&directory) {
                break *value;
            }
            visited.push(directory.clone());
            if self.indexed_directories.contains(&directory)
                || context
                    .root
                    .join(&directory)
                    .join(".gitattributes")
                    .exists()
            {
                break true;
            }
            if !directory.pop() {
                break false;
            }
        };
        // Cache only a bounded directory working set, not the repository's path catalogue.
        if self.directories.len() + visited.len() > 4096 {
            self.directories.clear();
        }
        for directory in visited {
            self.directories.insert(directory, result);
        }
        Ok(result)
    }
    fn conversions(
        &self,
        context: &Context<'_>,
        records: &[IndexRecord],
    ) -> Result<BTreeSet<Vec<u8>>, RepositoryError> {
        if self.always {
            return Ok(records.iter().map(|record| record.path.clone()).collect());
        }
        let mut input = Vec::new();
        for record in records {
            input.extend_from_slice(&record.path);
            input.push(0);
        }
        let mut affected = BTreeSet::new();
        // Both current rules and cached rules can explain why an index entry's
        // stat matches raw bytes that differ from its normalized object.
        for cached in [false, true] {
            let mut args = vec!["check-attr", "-z", "--stdin"];
            if cached {
                args.push("--cached");
            }
            args.extend(["filter", "text", "eol", "working-tree-encoding", "ident"]);
            let result = context.required(&args, Some(&input))?;
            let fields = result
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if fields.len() != records.len() * 15 {
                return Err(io_error("Unsupported Git attribute response"));
            }
            for triple in fields.chunks(3) {
                if !matches!(triple[2], b"unspecified" | b"unset") {
                    affected.insert(triple[0].to_vec());
                }
            }
        }
        Ok(affected)
    }
}

struct WorkingCapture<'a, 'b> {
    context: &'a Context<'b>,
    before: Entries,
    after: Entries,
    staged_paths: BTreeSet<Vec<u8>>,
    unstaged: bool,
    has_head: bool,
    sha256: bool,
    work_contents: &'a mut BTreeMap<Vec<u8>, Content>,
    observations: &'a mut BTreeMap<Vec<u8>, Option<Stamp>>,
    intents: &'a mut BTreeSet<Vec<u8>>,
    kept_bytes: usize,
    changed_count: usize,
}
impl WorkingCapture<'_, '_> {
    fn retain_index(&mut self, record: &IndexRecord) {
        if self.unstaged || (self.has_head && !self.staged_paths.contains(&record.path)) {
            self.before
                .insert(record.path.clone(), record.entry.clone());
        }
        self.after.insert(record.path.clone(), record.entry.clone());
    }
    fn freeze(&mut self, record: IndexRecord) -> Result<(), RepositoryError> {
        let intent = record.stat.flags & 0x2000_0000 != 0;
        if intent {
            self.intents.insert(record.path.clone());
        }
        if (self.unstaged || (self.has_head && !self.staged_paths.contains(&record.path)))
            && !intent
        {
            self.before
                .insert(record.path.clone(), record.entry.clone());
        }
        self.freeze_path(&record.path, Some(&record.entry))
    }
    fn freeze_path(&mut self, path: &[u8], prior: Option<&Entry>) -> Result<(), RepositoryError> {
        match work_file(self.context, path, prior, self.sha256)? {
            Some((entry, content, observed)) => {
                if self.before.get(path) != Some(&entry) || entry.conflict {
                    self.changed_count += 1;
                    self.kept_bytes += content.data.len();
                    if self.kept_bytes > MAX_SNAPSHOT {
                        return Err(budget(
                            "Changed worktree content exceeds the 64 MiB snapshot budget",
                        ));
                    }
                    self.work_contents.insert(path.to_vec(), content);
                    self.observations.insert(path.to_vec(), Some(observed));
                    self.after.insert(path.to_vec(), entry);
                } else {
                    // A stat false positive needs no retained catalogue entry or body.
                    self.before.remove(path);
                    self.after.remove(path);
                }
            }
            None => {
                self.after.remove(path);
                if self.before.contains_key(path) {
                    self.changed_count += 1;
                    self.observations.insert(path.to_vec(), None);
                }
            }
        }
        if self.changed_count > MAX_CHANGES {
            return Err(budget(
                "Comparison exceeds 2,000 changed paths; compare a smaller commit range",
            ));
        }
        Ok(())
    }
}

fn parse_entries(bytes: &[u8], index: bool) -> Result<Entries, RepositoryError> {
    let mut entries = Entries::new();
    for record in bytes
        .split(|&byte| byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|&byte| byte == b'\t')
            .ok_or_else(|| io_error("Malformed Git file listing"))?;
        let fields: Vec<_> = std::str::from_utf8(&record[..tab])
            .map_err(io_error)?
            .split_whitespace()
            .collect();
        if fields.len() != 3 {
            return Err(io_error("Malformed Git entry"));
        }
        if !matches!(fields[0], "100644" | "100755" | "120000" | "160000") {
            return Err(RepositoryError::Unsupported {
                message: format!("Unsupported Git entry mode: {}", fields[0]),
            });
        }
        let oid = fields[if index { 1 } else { 2 }];
        validate_oid(oid)?;
        let conflict = index && fields[2] != "0";
        let entry = Entry {
            mode: fields[0].into(),
            oid: oid.into(),
            conflict,
        };
        let path = record[tab + 1..].to_vec();
        validate_path(&path)?;
        entries
            .entry(path)
            .and_modify(|previous: &mut Entry| previous.conflict |= conflict)
            .or_insert(entry);
    }
    Ok(entries)
}

fn untracked(context: &Context<'_>) -> Result<Vec<Vec<u8>>, RepositoryError> {
    let bytes = context.required(&["ls-files", "--others", "--exclude-standard", "-z"], None)?;
    let paths: Vec<_> = bytes
        .split(|&byte| byte == 0)
        .filter(|path| !path.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    if paths.len() > MAX_CHANGES {
        return Err(budget("Too many untracked files"));
    }
    for path in &paths {
        validate_path(path)?;
    }
    Ok(paths)
}

fn validate_path(bytes: &[u8]) -> Result<(), RepositoryError> {
    let path = path_from_bytes(bytes)?;
    if bytes.is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(RepositoryError::Unsupported {
            message: "Git contains an unsafe path".into(),
        });
    }
    Ok(())
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, RepositoryError> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
}
#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, RepositoryError> {
    Ok(PathBuf::from(std::str::from_utf8(bytes).map_err(|_| {
        RepositoryError::Unsupported {
            message: "This platform cannot represent a non-UTF-8 Git path".into(),
        }
    })?))
}

fn display_path(path: &[u8]) -> String {
    // Escape control characters so a newline cannot impersonate another list row.
    String::from_utf8_lossy(path)
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

#[derive(Clone, Default)]
struct Content {
    data: Vec<u8>,
    truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    changed: Option<(i64, i64)>,
    identity: Option<(u64, u64)>,
    symlink: bool,
    file: bool,
    mode: u32,
}
fn stamp(metadata: &Metadata) -> Stamp {
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    };
    #[cfg(not(unix))]
    let mode = 0;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        Some((metadata.dev(), metadata.ino()))
    };
    #[cfg(not(unix))]
    let identity = None;
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        Some((metadata.ctime(), metadata.ctime_nsec()))
    };
    #[cfg(not(unix))]
    let changed = None;
    Stamp {
        changed,
        len: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        identity,
        symlink: metadata.is_symlink(),
        file: metadata.is_file(),
        mode,
    }
}

fn index_identity(context: &Context<'_>) -> Result<String, RepositoryError> {
    let path = context.required(
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        None,
    )?;
    let path = String::from_utf8(path).map_err(io_error)?;
    match File::open(path.trim_end()) {
        Ok(mut file) => {
            let mut hash = Sha256::new();
            let mut buffer = [0; 64 * 1024];
            loop {
                context.check()?;
                let n = file.read(&mut buffer).map_err(io_error)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            Ok(format!("{:x}", hash.finalize()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(digest(&[])),
        Err(error) => Err(io_error(error)),
    }
}

fn working_path(context: &Context<'_>, path: &[u8]) -> Result<PathBuf, RepositoryError> {
    let relative = path_from_bytes(path)?;
    let mut current = context.root.clone();
    let parts: Vec<_> = relative.components().collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_symlink() || !metadata.is_dir() => {
                return Err(RepositoryError::Unsupported {
                    message: format!(
                        "Refusing to traverse a symlink or non-directory parent: {}",
                        display_path(path)
                    ),
                })
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(context.root.join(relative))
}

fn work_file(
    context: &Context<'_>,
    path: &[u8],
    prior: Option<&Entry>,
    sha256: bool,
) -> Result<Option<(Entry, Content, Stamp)>, RepositoryError> {
    context.check()?;
    let full = working_path(context, path)?;
    let metadata = match fs::symlink_metadata(&full) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    let before = stamp(&metadata);
    if metadata.is_dir() {
        if prior.is_some_and(|entry| entry.mode == "160000") {
            // Nested Git operations are deliberately excluded; the gitlink remains explicit.
            return Ok(Some((
                prior.expect("checked prior").clone(),
                Content::default(),
                before,
            )));
        }
        return Err(RepositoryError::Unsupported {
            message: format!(
                "Expected a file but found a directory: {}",
                display_path(path)
            ),
        });
    }
    if !metadata.is_file() && !metadata.is_symlink() {
        return Err(RepositoryError::Unsupported {
            message: format!("Unsupported special file: {}", display_path(path)),
        });
    }
    let mut one = Sha1::new();
    let mut two = Sha256::new();
    let mut content = Content::default();
    let mode;
    let mut length = 0;
    if metadata.is_symlink() {
        let target = fs::read_link(&full).map_err(io_error)?;
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt;
            target.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let bytes = target.to_string_lossy().as_bytes().to_vec();
        let header = format!("blob {}\0", bytes.len());
        one.update(header.as_bytes());
        two.update(header.as_bytes());
        one.update(&bytes);
        two.update(&bytes);
        content.data = bytes;
        mode = "120000";
    } else {
        let header = format!("blob {}\0", metadata.len());
        one.update(header.as_bytes());
        two.update(header.as_bytes());
        let mut file = File::open(&full).map_err(io_error)?;
        // Compare the handle after opening as well as the pathname to catch replacement.
        if stamp(&file.metadata().map_err(io_error)?) != before {
            return Err(stale("File changed while it was opened"));
        }
        let mut buffer = [0; 32 * 1024];
        loop {
            context.check()?;
            let size = file.read(&mut buffer).map_err(io_error)?;
            if size == 0 {
                break;
            }
            length += size as u64;
            if length > 512 * 1024 * 1024 {
                return Err(budget("A worktree file exceeds the 512 MiB hashing limit"));
            }
            one.update(&buffer[..size]);
            two.update(&buffer[..size]);
            let keep = size.min(MAX_FILE.saturating_sub(content.data.len()));
            content.data.extend_from_slice(&buffer[..keep]);
            content.truncated |= keep < size;
        }
        if length != metadata.len() {
            return Err(stale("File size changed during capture"));
        }
        #[cfg(unix)]
        {
            mode = if before.mode & 0o111 != 0 {
                "100755"
            } else {
                "100644"
            };
        }
        #[cfg(not(unix))]
        {
            mode = prior
                .map(|entry| entry.mode.as_str())
                .filter(|mode| *mode == "100755")
                .unwrap_or("100644");
        }
    }
    if stamp(&fs::symlink_metadata(&full).map_err(io_error)?) != before {
        return Err(stale("File changed during capture"));
    }
    let oid = if sha256 {
        format!("{:x}", two.finalize())
    } else {
        format!("{:x}", one.finalize())
    };
    Ok(Some((
        Entry {
            mode: mode.into(),
            oid,
            conflict: prior.is_some_and(|entry| entry.conflict),
        },
        content,
        before,
    )))
}

fn blobs(
    context: &Context<'_>,
    entries: impl Iterator<Item = Entry>,
) -> Result<BTreeMap<String, Content>, RepositoryError> {
    let oids: BTreeSet<_> = entries
        .filter(|entry| entry.mode != "160000")
        .map(|entry| entry.oid)
        .collect();
    if oids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let request = oids
        .iter()
        .map(|oid| format!("{oid}\n"))
        .collect::<String>();
    let checked = context.required(
        &[
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ],
        Some(request.as_bytes()),
    )?;
    let mut result = BTreeMap::new();
    let mut readable = Vec::new();
    let mut total = 0;
    for line in std::str::from_utf8(&checked).map_err(io_error)?.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 || fields[1] != "blob" {
            return Err(io_error(format!("Unavailable Git blob: {line}")));
        }
        let length: usize = fields[2].parse().map_err(io_error)?;
        if length > MAX_FILE {
            result.insert(
                fields[0].into(),
                Content {
                    data: Vec::new(),
                    truncated: true,
                },
            );
        } else {
            total += length;
            readable.push(fields[0].to_owned());
        }
        if total > MAX_SNAPSHOT {
            return Err(budget("Changed blobs exceed the 64 MiB snapshot budget"));
        }
    }
    if readable.is_empty() {
        return Ok(result);
    }
    let request = readable
        .iter()
        .map(|oid| format!("{oid}\n"))
        .collect::<String>();
    let output = context.required(&["cat-file", "--batch"], Some(request.as_bytes()))?;
    let mut rest = output.as_slice();
    for expected in readable {
        let newline = rest
            .iter()
            .position(|&byte| byte == b'\n')
            .ok_or_else(|| io_error("Truncated Git batch header"))?;
        let fields: Vec<_> = std::str::from_utf8(&rest[..newline])
            .map_err(io_error)?
            .split_whitespace()
            .collect();
        if fields.len() != 3 || fields[0] != expected || fields[1] != "blob" {
            return Err(io_error("Unexpected Git batch object"));
        }
        let length: usize = fields[2].parse().map_err(io_error)?;
        rest = &rest[newline + 1..];
        if length > MAX_FILE || rest.len() <= length || rest[length] != b'\n' {
            return Err(io_error("Truncated Git batch contents"));
        }
        result.insert(
            expected,
            Content {
                data: rest[..length].to_vec(),
                truncated: false,
            },
        );
        rest = &rest[length + 1..];
    }
    if !rest.is_empty() {
        return Err(io_error("Unexpected trailing Git batch data"));
    }
    Ok(result)
}

/// A captured comparison contains only immutable, bounded data. No methods reread the repository.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenComparison {
    manifest: ComparisonManifest,
    files: BTreeMap<String, FrozenFile>,
}
impl FrozenComparison {
    /// Validate deserialized snapshots before they can become review evidence.
    pub(crate) fn validate_stored(&self) -> Result<(), RepositoryError> {
        fn valid_id(id: &str, lengths: &[usize]) -> bool {
            lengths.contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        if !valid_id(&self.manifest.id, &[64])
            || self.files.len() > MAX_CHANGES
            || self.manifest.total_files as usize != self.files.len()
            || self.manifest.files.len() != self.files.len()
        {
            return Err(io_error("Invalid frozen comparison manifest"));
        }
        let mut seen = BTreeSet::new();
        let mut bytes = 0;
        for summary in &self.manifest.files {
            if !valid_id(&summary.id, &[64]) || !seen.insert(&summary.id) {
                return Err(io_error("Duplicate or invalid frozen file identity"));
            }
            let file = self
                .files
                .get(&summary.id)
                .ok_or_else(|| io_error("Frozen file is missing"))?;
            if serde_json::to_vec(summary).map_err(io_error)?
                != serde_json::to_vec(&file.summary).map_err(io_error)?
            {
                return Err(io_error("Frozen file summary does not match its manifest"));
            }
            for (value, truncated, oid, mode) in [
                (
                    &file.before,
                    file.before_truncated,
                    &summary.before_id,
                    &summary.old_mode,
                ),
                (
                    &file.after,
                    file.after_truncated,
                    &summary.after_id,
                    &summary.new_mode,
                ),
            ] {
                if oid.as_ref().is_some_and(|oid| !valid_id(oid, &[40, 64]))
                    || mode.as_ref().is_some_and(|mode| {
                        !matches!(mode.as_str(), "100644" | "100755" | "120000" | "160000")
                    })
                    || oid.is_some() != mode.is_some()
                {
                    return Err(io_error("Invalid frozen source identity or mode"));
                }
                if let Some(text) = value {
                    if text.len() > MAX_FILE || oid.is_none() {
                        return Err(io_error("Invalid frozen source content"));
                    }
                    bytes += text.len();
                    if !truncated {
                        let oid = oid.as_deref().expect("checked source identity");
                        if mode.as_deref() == Some("160000") {
                            if text != &format!("Submodule commit {oid}\n") {
                                return Err(io_error("Invalid frozen gitlink preview"));
                            }
                        } else {
                            let mut raw = format!("blob {}\0", text.len()).into_bytes();
                            raw.extend_from_slice(text.as_bytes());
                            let actual = if oid.len() == 40 {
                                format!("{:x}", Sha1::digest(&raw))
                            } else {
                                format!("{:x}", Sha256::digest(&raw))
                            };
                            if actual != oid {
                                return Err(io_error("Frozen source checksum mismatch"));
                            }
                        }
                    }
                }
            }
        }
        if bytes > MAX_SNAPSHOT {
            return Err(budget("Frozen comparison exceeds its content budget"));
        }
        Ok(())
    }
    pub fn manifest(&self, offset: usize, limit: usize) -> ComparisonManifest {
        let mut manifest = self.manifest.clone();
        manifest.files = manifest
            .files
            .into_iter()
            .skip(offset)
            .take(limit.min(MAX_CHANGES))
            .collect();
        manifest
    }
    pub fn read_file(&self, id: &str) -> Result<FrozenFile, RepositoryError> {
        self.files
            .get(id)
            .cloned()
            .ok_or_else(|| stale("File does not belong to this comparison"))
    }
}

/// Capture once; a stale working tree is reported instead of silently combining generations.
pub fn capture(
    spec: &ComparisonSpec,
    cancel: &AtomicBool,
) -> Result<FrozenComparison, RepositoryError> {
    let root =
        fs::canonicalize(&spec.repository).map_err(|error| RepositoryError::InvalidRepository {
            message: error.to_string(),
        })?;
    let mut context = Context {
        root,
        cancel,
        deadline: Instant::now() + CAPTURE_BUDGET,
    };
    let version = context.git(&["--version"], None)?;
    if !version.success {
        return Err(RepositoryError::GitUnavailable {
            message: "Diffkin requires Git 2.45 or newer with --no-lazy-fetch support".into(),
        });
    }
    let discovered = context.git(&["rev-parse", "--show-toplevel"], None)?;
    if !discovered.success {
        return Err(RepositoryError::InvalidRepository {
            message: discovered.stderr,
        });
    }
    let root = path_from_bytes(
        discovered
            .stdout
            .strip_suffix(b"\n")
            .unwrap_or(&discovered.stdout),
    )?;
    context.root = fs::canonicalize(root).map_err(io_error)?;
    let version = String::from_utf8_lossy(&version.stdout);
    let numbers: Vec<u32> = version
        .split_whitespace()
        .nth(2)
        .unwrap_or("")
        .split('.')
        .take(2)
        .map(|number| number.parse().unwrap_or(0))
        .collect();
    if numbers.as_slice() < [2, 45].as_slice() {
        return Err(RepositoryError::GitUnavailable {
            message: "Diffkin requires Git 2.45 or newer".into(),
        });
    }
    let mut warnings = Vec::new();
    let index_before = if spec.mode != CompareMode::Commits {
        Some(index_identity(&context)?)
    } else {
        None
    };
    let before_label;
    let after_label;
    let before;
    let mut after;
    let mut work_contents = BTreeMap::new();
    let mut observations = BTreeMap::new();
    let mut index_observed = None;
    let mut untracked_before = Vec::new();
    let mut intent_paths = BTreeSet::new();
    if spec.mode == CompareMode::Commits {
        let base = context.resolve(spec.base.as_deref().unwrap_or("HEAD~1"))?;
        let head = context.resolve(spec.head.as_deref().unwrap_or("HEAD"))?;
        (before, after, _) = raw_diff(&context, Some(&base), Some(&head))?;
        before_label = base;
        after_label = head;
    } else {
        let head = context.head()?;
        let (staged_before, staged_after, staged_paths) = if spec.mode == CompareMode::Unstaged {
            (Entries::new(), Entries::new(), BTreeSet::new())
        } else {
            raw_diff(&context, head.as_deref(), None)?
        };
        before_label = if spec.mode == CompareMode::Unstaged {
            format!("index:{}", index_before.as_deref().unwrap_or_default())
        } else {
            head.clone()
                .unwrap_or_else(|| "unborn HEAD (empty tree)".into())
        };
        if spec.mode == CompareMode::Staged {
            before = staged_before;
            after = staged_after;
            scan_index(&context, |record| {
                if staged_paths.contains(&record.path) {
                    if record.stat.flags & 0x2000_0000 != 0 {
                        intent_paths.insert(record.path.clone());
                        after.remove(&record.path);
                    } else {
                        after.insert(record.path, record.entry);
                    }
                }
                Ok(())
            })?;
            after_label = format!("index:{}", index_before.as_deref().unwrap_or_default());
        } else {
            let sha256 = String::from_utf8_lossy(
                &context.required(&["rev-parse", "--show-object-format"], None)?,
            )
            .trim()
                == "sha256";
            let index_time = index_modified(&context)?;
            let mut attributes = Attributes::new(&context)?;
            if staged_paths.iter().any(|path| {
                path.rsplit(|byte| *byte == b'/').next() == Some(b".gitattributes".as_slice())
            }) {
                // A staged rule change may remove the only description of an
                // earlier normalization. Verify this capture literally.
                attributes.always = true;
            }
            let mut capture = WorkingCapture {
                context: &context,
                before: if spec.mode == CompareMode::Unstaged {
                    Entries::new()
                } else {
                    staged_before
                },
                after: if spec.mode == CompareMode::Unstaged {
                    Entries::new()
                } else {
                    staged_after
                },
                staged_paths,
                unstaged: spec.mode == CompareMode::Unstaged,
                has_head: head.is_some(),
                sha256,
                work_contents: &mut work_contents,
                observations: &mut observations,
                intents: &mut intent_paths,
                kept_bytes: 0,
                changed_count: 0,
            };
            let mut observed = Sha256::new();
            let mut possible_conversions = Vec::new();
            let mut batch_bytes = 0;
            scan_index(&context, |record| {
                let metadata = observe(&context, &record.path, &mut observed)?;
                // Sparse absence is context, not a deletion. Existing sparse files are checked.
                if metadata.is_none() && record.stat.flags & 0x4000_0000 != 0 {
                    if capture.staged_paths.contains(&record.path) {
                        capture.retain_index(&record);
                    }
                    return Ok(());
                }
                if spec.verify_all
                    || capture.staged_paths.contains(&record.path)
                    || !metadata
                        .as_ref()
                        .is_some_and(|meta| index_matches(&record, meta, index_time))
                {
                    capture.freeze(record)?;
                } else if attributes.possible(&context, &record.path)? {
                    batch_bytes += record.path.len();
                    possible_conversions.push(record);
                    if possible_conversions.len() >= 1024 || batch_bytes >= 64 * 1024 {
                        let affected = attributes.conversions(&context, &possible_conversions)?;
                        for record in possible_conversions.drain(..) {
                            if affected.contains(&record.path) {
                                capture.freeze(record)?;
                            }
                        }
                        batch_bytes = 0;
                    }
                }
                Ok(())
            })?;
            if !possible_conversions.is_empty() {
                let affected = attributes.conversions(&context, &possible_conversions)?;
                for record in possible_conversions {
                    if affected.contains(&record.path) {
                        capture.freeze(record)?;
                    }
                }
            }
            index_observed = Some(format!("{:x}", observed.finalize()));
            if spec.include_untracked {
                untracked_before = untracked(&context)?;
            }
            for path in &untracked_before {
                capture.freeze_path(path, None)?;
            }
            before = capture.before;
            after = capture.after;
            #[cfg(windows)]
            warnings.push("Windows executable bits are inherited from the index. A symlink checked out as a regular file is shown as a literal file-type change.".into());
            warnings.push("Working-tree bytes are captured literally; clean/smudge filters and line-ending conversion are not executed. Capture is not a filesystem-wide atomic transaction.".into());
            warnings.push(if spec.verify_all {
                "Full literal verification: all existing tracked file bytes were checked within the capture budget; absent sparse files retain index context."
            } else {
                "Fast capture uses Git index/stat metadata for unchanged tracked files. Changed, flagged, racy or known conversion-affected files are hashed literally. Historical conversion rules may no longer be discoverable; enable full literal verification to check all existing tracked bytes."
            }.into());
            if before
                .values()
                .chain(after.values())
                .any(|entry| entry.mode == "160000")
            {
                warnings.push("Submodule working directories are not traversed; nested dirty state is not captured. Gitlink changes remain visible in staged/commit comparisons.".into());
            }
            after_label = format!(
                "worktree:{}:{}",
                index_before.as_deref().unwrap_or_default(),
                entry_identity(&after)
            );
        }
    }
    let mut changed: Vec<_> = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| {
            before.get(path) != after.get(path)
                || before.get(path).is_some_and(|entry| entry.conflict)
                || after.get(path).is_some_and(|entry| entry.conflict)
        })
        .collect();
    if changed.len() > MAX_CHANGES {
        return Err(budget(
            "Comparison exceeds 2,000 changed paths; compare a smaller commit range",
        ));
    }
    let needed = changed.iter().flat_map(|path| {
        before.get(path).cloned().into_iter().chain(
            if matches!(spec.mode, CompareMode::Commits | CompareMode::Staged) {
                after.get(path).cloned()
            } else {
                None
            },
        )
    });
    let blob_contents = blobs(&context, needed)?;
    let mut total = blob_contents
        .values()
        .map(|content| content.data.len())
        .sum::<usize>()
        + work_contents
            .values()
            .map(|content| content.data.len())
            .sum::<usize>();
    if total > MAX_SNAPSHOT {
        return Err(budget("Comparison exceeds the 64 MiB snapshot budget"));
    }
    // Only unique, exactly identical objects are paired as path renames.
    let mut deleted: BTreeMap<(String, String), Vec<Vec<u8>>> = BTreeMap::new();
    let mut added: BTreeMap<(String, String), Vec<Vec<u8>>> = BTreeMap::new();
    for path in &changed {
        match (before.get(path), after.get(path)) {
            (Some(old), None) if !old.conflict => deleted
                .entry((old.oid.clone(), old.mode.clone()))
                .or_default()
                .push(path.clone()),
            (None, Some(new)) if !new.conflict => added
                .entry((new.oid.clone(), new.mode.clone()))
                .or_default()
                .push(path.clone()),
            _ => (),
        }
    }
    let mut renames = BTreeMap::new();
    let mut renamed_old = BTreeSet::new();
    for (key, destinations) in &added {
        if let Some(origins) = deleted.get(key) {
            if origins.len() == 1 && destinations.len() == 1 {
                renames.insert(destinations[0].clone(), origins[0].clone());
                renamed_old.insert(origins[0].clone());
            }
        }
    }
    changed.retain(|path| !renamed_old.contains(path));
    let mut files = BTreeMap::new();
    let mut summaries = Vec::new();
    for path in changed {
        context.check()?;
        let old_path = renames.get(&path).unwrap_or(&path);
        let old = before.get(old_path);
        let new = after.get(&path);
        let mut notices = Vec::new();
        if intent_paths.contains(&path) {
            notices.push("Intent-to-add: the index placeholder has no staged content; this comparison shows the working-tree addition.".into());
        }
        if std::str::from_utf8(&path).is_err() || std::str::from_utf8(old_path).is_err() {
            notices.push("Path contains non-UTF-8 bytes; the display uses replacement characters, while IDs retain the original bytes.".into());
        }
        if old.is_some_and(|entry| entry.conflict) || new.is_some_and(|entry| entry.conflict) {
            notices.push("Unresolved index conflict. This view shows an available index stage and the captured counterpart, not a resolved merge.".into());
        }
        let status =
            if old.is_some_and(|entry| entry.conflict) || new.is_some_and(|entry| entry.conflict) {
                FileStatus::Conflict
            } else if renames.contains_key(&path) {
                FileStatus::Renamed
            } else {
                match (old, new) {
                    (None, _) => FileStatus::Added,
                    (_, None) => FileStatus::Deleted,
                    (Some(a), Some(b)) if a.mode[..3] != b.mode[..3] => FileStatus::TypeChanged,
                    _ => FileStatus::Modified,
                }
            };
        if old.map(|entry| &entry.mode) != new.map(|entry| &entry.mode)
            && old.is_some()
            && new.is_some()
        {
            notices.push("File mode changed.".into());
        }
        let old_content = old.and_then(|entry| blob_contents.get(&entry.oid));
        let new_content = if matches!(spec.mode, CompareMode::Commits | CompareMode::Staged) {
            new.and_then(|entry| blob_contents.get(&entry.oid))
        } else {
            work_contents.get(&path)
        };
        let (old_text, before_truncated) = text_content(old, old_content, "Before", &mut notices);
        let (new_text, after_truncated) = text_content(new, new_content, "After", &mut notices);
        let mut identity = Sha256::new();
        identity.update(&path);
        identity.update([0]);
        identity.update(old_path);
        if let Some(entry) = old {
            identity.update(entry.oid.as_bytes());
            identity.update(entry.mode.as_bytes());
        }
        if let Some(entry) = new {
            identity.update(entry.oid.as_bytes());
            identity.update(entry.mode.as_bytes());
        }
        let id = format!("{:x}", identity.finalize());
        let summary = FileSummary {
            id: id.clone(),
            path: display_path(&path),
            old_path: renames.get(&path).map(|path| display_path(path)),
            status,
            old_mode: old.map(|entry| entry.mode.clone()),
            new_mode: new.map(|entry| entry.mode.clone()),
            before_id: old.map(|entry| entry.oid.clone()),
            after_id: new.map(|entry| entry.oid.clone()),
            notices,
        };
        total +=
            old_text.as_ref().map_or(0, String::len) + new_text.as_ref().map_or(0, String::len);
        if total > MAX_SNAPSHOT * 2 {
            return Err(budget("Decoded comparison exceeds the memory budget"));
        }
        summaries.push(summary.clone());
        files.insert(
            id,
            FrozenFile {
                summary,
                before: old_text,
                after: new_text,
                before_truncated,
                after_truncated,
            },
        );
    }
    // Recheck inputs before publishing. Immutable objects need no second reads.
    if let Some(previous) = index_before {
        if index_identity(&context)? != previous {
            return Err(stale(
                "Git index changed during capture; refresh the comparison",
            ));
        }
    }
    if let Some(expected) = index_observed {
        if index_observation(&context)? != expected {
            return Err(stale(
                "Working tree metadata changed during capture; refresh the comparison",
            ));
        }
    }
    for (path, observed) in observations {
        context.check()?;
        let full = working_path(&context, &path)?;
        let current = match fs::symlink_metadata(full) {
            Ok(metadata) => Some(stamp(&metadata)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(io_error(error)),
        };
        if current != observed {
            return Err(stale(
                "Working tree changed during capture; refresh the comparison",
            ));
        }
    }
    if spec.include_untracked
        && matches!(spec.mode, CompareMode::Worktree | CompareMode::Unstaged)
        && untracked(&context)? != untracked_before
    {
        return Err(stale("Untracked file list changed during capture"));
    }
    let mut identity = Sha256::new();
    identity.update(context.root.as_os_str().to_string_lossy().as_bytes());
    identity.update(before_label.as_bytes());
    identity.update(after_label.as_bytes());
    for file in &summaries {
        identity.update(file.id.as_bytes());
    }
    let id = format!("{:x}", identity.finalize());
    Ok(FrozenComparison {
        manifest: ComparisonManifest {
            id,
            repository: context.root.to_string_lossy().into_owned(),
            mode: spec.mode,
            base_label: before_label,
            head_label: after_label,
            total_files: summaries.len() as u32,
            files: summaries,
            warnings,
        },
        files,
    })
}

fn entry_identity(entries: &Entries) -> String {
    let mut hasher = Sha256::new();
    for (path, entry) in entries {
        hasher.update(path);
        hasher.update([0]);
        hasher.update(entry.mode.as_bytes());
        hasher.update(entry.oid.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn text_content(
    entry: Option<&Entry>,
    content: Option<&Content>,
    side: &str,
    notices: &mut Vec<String>,
) -> (Option<String>, bool) {
    let Some(entry) = entry else {
        return (None, false);
    };
    if entry.mode == "160000" {
        notices.push(format!("{side}: submodule gitlink {}", entry.oid));
        return (Some(format!("Submodule commit {}\n", entry.oid)), false);
    }
    if entry.mode == "120000" {
        notices.push(format!(
            "{side}: symbolic link target; the link is not followed."
        ));
    }
    let Some(content) = content else {
        notices.push(format!("{side}: content unavailable."));
        return (None, false);
    };
    if content.truncated {
        notices.push(if content.data.is_empty() { format!("{side}: large Git blob; its object identity is frozen, but a text preview was not captured.") } else { format!("{side}: large file; only a bounded preview is available and structure analysis is disabled.") });
    }
    if content.data.contains(&0) {
        notices.push(format!(
            "{side}: binary data; text analysis is unavailable."
        ));
        return (None, content.truncated);
    }
    // A truncated UTF-8 preview can end inside a multibyte character; trim only that suffix.
    let data = match std::str::from_utf8(&content.data) {
        Ok(text) => text,
        Err(error) if content.truncated && error.error_len().is_none() => {
            std::str::from_utf8(&content.data[..error.valid_up_to()]).expect("valid prefix")
        }
        Err(_) => {
            notices.push(format!(
                "{side}: non-UTF-8 bytes; no lossy text conversion is performed."
            ));
            return (None, content.truncated);
        }
    };
    if data.starts_with("version https://git-lfs.github.com/spec/v1\n")
        || data.starts_with("version https://git-lfs.github.com/spec/v1\r\n")
    {
        notices.push(format!(
            "{side}: Git LFS pointer; external LFS content is not downloaded."
        ));
    }
    (Some(data.into()), content.truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_object_placeholder_is_rejected_before_blob_reads() {
        let record = b"100644 0000000000000000000000000000000000000000 0\tplaceholder\0";
        assert!(matches!(
            parse_entries(record, true),
            Err(RepositoryError::IoFailure { .. })
        ));
    }
}
