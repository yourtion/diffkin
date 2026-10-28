//! Explicitly approved, trusted-local command execution. This is not a sandbox.
use diffkin_domain::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
const MAX_FILES: usize = 10_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILE: usize = 8 * 1024 * 1024;
const MAX_LOG: usize = 1024 * 1024;
const TAIL: usize = 64 * 1024;
const FIXED_PATH: &str = "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin";
type Result<T> = std::result::Result<T, ValidationError>;
fn invalid(s: impl Into<String>) -> ValidationError {
    ValidationError::Invalid { message: s.into() }
}
fn io(e: impl std::fmt::Display) -> ValidationError {
    ValidationError::Io {
        message: e.to_string(),
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
}
fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(ValidationError::Cancelled {
            message: "Validation cancelled".into(),
        })
    } else {
        Ok(())
    }
}
fn valid_id(id: &str) -> Result<()> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| invalid("Invalid record identifier"))
}
fn relative(path: &str, empty: bool) -> bool {
    (empty && (path.is_empty() || path == "."))
        || (!path.is_empty()
            && !path.contains('\\')
            && path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".." && !p.eq_ignore_ascii_case(".git"))
            && Path::new(path)
                .components()
                .all(|c| matches!(c, Component::Normal(_))))
}
fn real_directory(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(invalid(
            "Validation storage directories must not be symbolic links",
        ));
    }
    Ok(())
}
fn create_directory(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => real_directory(path),
        Err(e) => Err(io(e)),
    }
}
fn source_parent(root: &Path, path: &str) -> Result<()> {
    let mut parent = root.to_path_buf();
    let directory = Path::new(path).parent().unwrap_or(Path::new(""));
    for component in directory.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("Unsafe source directory"));
        };
        let next = parent.join(name);
        create_directory(&next)?;
        let exact = fs::read_dir(&parent)
            .map_err(io)?
            .filter_map(|e| e.ok())
            .any(|e| e.file_name() == name);
        if !exact {
            return Err(invalid(
                "Source directories alias under filesystem case or Unicode normalization",
            ));
        }
        parent = next;
    }
    Ok(())
}
fn future_location(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(io)?.join(path)
    };
    if absolute
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(invalid("Storage path cannot contain parent traversal"));
    }
    let mut ancestor = absolute.as_path();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .ok_or_else(|| invalid("Invalid storage path"))?
                .to_owned(),
        );
        ancestor = ancestor
            .parent()
            .ok_or_else(|| invalid("Invalid storage parent"))?;
    }
    let mut resolved = fs::canonicalize(ancestor).map_err(io)?;
    for name in missing.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}
fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(value).map_err(io)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(io)?;
    file.write_all(&bytes).map_err(io)?;
    file.sync_all().map_err(io)?;
    fs::rename(tmp, path).map_err(io)
}
fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path).map_err(io)?;
    if file.metadata().map_err(io)?.len() > 16 * 1024 * 1024 {
        return Err(invalid("Record exceeds limit"));
    }
    serde_json::from_reader(file).map_err(io)
}
#[derive(Serialize, Deserialize)]
struct Entry {
    path: String,
    digest: String,
    executable: bool,
    size: usize,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    entries: Vec<Entry>,
}
fn file_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(io)?;
    if !file.metadata().map_err(io)?.is_file() {
        return Err(invalid("Program is not a regular file"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut total = 0;
    loop {
        let n = file.read(&mut buffer).map_err(io)?;
        if n == 0 {
            break;
        }
        total += n;
        if total > 256 * 1024 * 1024 {
            return Err(invalid("Executable exceeds 256 MiB"));
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn resolve_program(value: &str) -> Result<PathBuf> {
    let p = Path::new(value);
    let path = if p.is_absolute() {
        p.to_path_buf()
    } else if p.components().count() == 1 {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(p))
            .find(|p| p.is_file())
            .ok_or_else(|| invalid("Program is not available on PATH"))?
    } else {
        return Err(invalid(
            "Program must be absolute or a PATH executable name",
        ));
    };
    fs::canonicalize(path).map_err(io)
}
fn git(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(root)
        .args([
            "--no-pager",
            "--no-replace-objects",
            "--no-lazy-fetch",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "protocol.allow=never",
        ])
        .env_clear()
        .env("PATH", FIXED_PATH)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LC_ALL", "C");
    c
}
fn git_output(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    deadline: Instant,
    limit: usize,
) -> Result<Vec<u8>> {
    check(cancel)?;
    if Instant::now() >= deadline {
        return Err(invalid("Candidate assembly exceeded its 30-second budget"));
    }
    let mut command = git(root);
    command.args(args);
    let mut output = Vec::new();
    let mut overflow = false;
    let outcome = process(&mut command, cancel, deadline, |stream, bytes| {
        if stream == "stdout" {
            if output.len() + bytes.len() <= limit {
                output.extend_from_slice(bytes);
            } else {
                overflow = true;
            }
        }
    });
    check(cancel)?;
    let outcome = outcome.map_err(io)?;
    if overflow || outcome.timed_out || outcome.code != Some(0) {
        return Err(invalid(
            "Git object read failed, timed out, or exceeded budget; no fetch was attempted",
        ));
    }
    Ok(output)
}
fn git_text(root: &Path, args: &[&str], cancel: &AtomicBool, deadline: Instant) -> Result<String> {
    String::from_utf8(git_output(root, args, cancel, deadline, 4096)?)
        .map(|s| s.trim().into())
        .map_err(io)
}
fn assemble(
    repository: &Path,
    reference: &str,
    root: &Path,
    cancel: &AtomicBool,
    deadline: Instant,
    blockers: &mut Vec<String>,
) -> Result<ValidationSnapshot> {
    if reference.is_empty() || reference.len() > 1024 || reference.starts_with('-') {
        return Err(invalid("Invalid commit reference"));
    }
    let commit = git_text(
        repository,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
        cancel,
        deadline,
    )?;
    let tree = git_text(
        repository,
        &["rev-parse", "--verify", &format!("{commit}^{{tree}}")],
        cancel,
        deadline,
    )?;
    let listing = git_output(
        repository,
        &["ls-tree", "-r", "-z", "-l", "--full-tree", &commit],
        cancel,
        deadline,
        4 * 1024 * 1024,
    )?;
    fs::create_dir_all(root).map_err(io)?;
    let mut entries = Vec::new();
    let mut locks = Vec::new();
    let mut total = 0;
    let initial = blockers.len();
    for record in listing.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        check(cancel)?;
        if entries.len() >= MAX_FILES {
            return Err(invalid("Candidate exceeds 10000 files"));
        }
        let Some(tab) = record.iter().position(|b| *b == b'\t') else {
            return Err(invalid("Invalid Git tree record"));
        };
        let header = std::str::from_utf8(&record[..tab]).map_err(io)?;
        let fields: Vec<_> = header.split_whitespace().collect();
        let path = match std::str::from_utf8(&record[tab + 1..]) {
            Ok(p) if relative(p, false) => p,
            _ => {
                blockers.push("Unsupported or non-UTF8 source path".into());
                continue;
            }
        };
        if fields.len() != 4 || !matches!(fields[0], "100644" | "100755") || fields[1] != "blob" {
            blockers.push(format!(
                "{path}: symbolic links and submodules are not assembled"
            ));
            continue;
        }
        let size: usize = fields[3].parse().map_err(io)?;
        total += size;
        if size > MAX_FILE || total > MAX_BYTES {
            return Err(invalid("Candidate exceeds 8 MiB per file or 64 MiB total"));
        }
        let bytes = git_output(
            repository,
            &["cat-file", "blob", fields[2]],
            cancel,
            deadline,
            MAX_FILE,
        )?;
        if bytes.len() != size {
            return Err(invalid("Git blob length changed"));
        }
        if bytes.starts_with(b"version https://git-lfs.github.com/spec/v1") {
            blockers.push(format!(
                "{path}: LFS content is unavailable; pointer is not a complete candidate"
            ));
        }
        if Path::new(path)
            .file_name()
            .is_some_and(|n| n == ".gitattributes")
            && String::from_utf8_lossy(&bytes)
                .split_whitespace()
                .any(|t| t.starts_with("filter=") || t.starts_with("working-tree-encoding="))
        {
            blockers.push(format!("{path}: checkout transformations are not executed"));
        }
        let target = root.join(path);
        source_parent(root, path)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| {
                invalid(format!(
                    "Cannot materialize distinct source path {path}: {e}"
                ))
            })?;
        file.write_all(&bytes).map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if fields[0] == "100755" { 0o755 } else { 0o644 }),
            )
            .map_err(io)?;
        }
        let hash = digest(&bytes);
        if is_lock(path) {
            locks.push(ValidationIdentity {
                path: path.into(),
                digest: hash.clone(),
            });
        }
        entries.push(Entry {
            path: path.into(),
            digest: hash,
            executable: fields[0] == "100755",
            size,
        });
    }
    let manifest = Manifest { entries };
    let content_id = digest(&serde_json::to_vec(&manifest).map_err(io)?);
    save(&root.with_extension("manifest.json"), &manifest)?;
    let mut snapshot = ValidationSnapshot {
        commit,
        tree,
        content_id,
        root: root.to_string_lossy().into(),
        file_count: manifest.entries.len() as u32,
        total_bytes: total as u32,
        locks,
        complete: initial == blockers.len(),
    };
    if !verify(&snapshot, cancel)?.is_empty() {
        snapshot.complete = false;
        blockers.push("Materialized source differs from the full Git tree (for example filesystem case collisions)".into());
    }
    Ok(snapshot)
}
fn is_lock(path: &str) -> bool {
    matches!(
        Path::new(path).file_name().and_then(|s| s.to_str()),
        Some(
            "Cargo.lock"
                | "pnpm-lock.yaml"
                | "package-lock.json"
                | "yarn.lock"
                | "bun.lock"
                | "go.sum"
                | "poetry.lock"
                | "uv.lock"
                | "Pipfile.lock"
                | "gradle.lockfile"
                | "Gemfile.lock"
                | "composer.lock"
        )
    )
}
/// Assemble a complete fixed-commit candidate without executing project code.
pub fn prepare(spec: &ValidationSpec, store: &Path, cancel: &AtomicBool) -> Result<ValidationPlan> {
    check(cancel)?;
    if !cfg!(unix) {
        return Err(ValidationError::Unsupported{message:"Trusted command adapter requires Unix process groups; Windows execution is unavailable".into()});
    }
    if !(1..=3600).contains(&spec.timeout_seconds)
        || spec.args.len() > 256
        || spec.args.iter().map(|s| s.len()).sum::<usize>() > 65536
        || !relative(&spec.working_directory, true)
    {
        return Err(invalid(
            "Invalid timeout, arguments or relative working directory",
        ));
    }
    let requested = fs::canonicalize(&spec.repository).map_err(io)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let top = git_text(
        &requested,
        &["rev-parse", "--show-toplevel"],
        cancel,
        deadline,
    )?;
    let repository = fs::canonicalize(top).map_err(io)?;
    let program = resolve_program(&spec.program)?;
    let program_id = file_hash(&program)?;
    let store = future_location(store)?;
    if store.starts_with(&repository) {
        return Err(invalid(
            "Validation store must be outside the user repository",
        ));
    }
    fs::create_dir_all(&store).map_err(io)?;
    real_directory(&store)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    create_directory(&store.join("plans"))?;
    create_directory(&store.join("runs"))?;
    let id = Uuid::new_v4().to_string();
    let dir = store.join("plans").join(&id);
    fs::create_dir(&dir).map_err(io)?;
    let mut blockers = Vec::new();
    let candidate = assemble(
        &repository,
        &spec.candidate_ref,
        &dir.join("candidate"),
        cancel,
        deadline,
        &mut blockers,
    )?;
    let baseline = spec
        .baseline_ref
        .as_ref()
        .map(|reference| {
            assemble(
                &repository,
                reference,
                &dir.join("baseline"),
                cancel,
                deadline,
                &mut blockers,
            )
        })
        .transpose()?;
    for snapshot in std::iter::once(&candidate).chain(baseline.iter()) {
        if !Path::new(&snapshot.root)
            .join(&spec.working_directory)
            .is_dir()
        {
            blockers.push(format!(
                "Working directory does not exist in {}",
                snapshot.commit
            ));
        }
    }
    let command_id = digest(
        &serde_json::to_vec(&(
            &program,
            &program_id,
            &spec.args,
            &spec.working_directory,
            spec.timeout_seconds,
            FIXED_PATH,
            "isolated-home-tmp-ci-c-v1",
        ))
        .map_err(io)?,
    );
    let plan=ValidationPlan {
        id, created_at_ms: now(), repository: repository.to_string_lossy().into(),
        program: program.to_string_lossy().into(), program_id, args: spec.args.clone(),
        working_directory: spec.working_directory.clone(), timeout_seconds: spec.timeout_seconds,
        command_id, candidate, baseline, executable: blockers.is_empty(), blockers,
        dependency_status: "unconfirmed".into(),
        limitations: vec![
            "Trusted local commands are not a security sandbox and can access the host; only execute trusted commands.".into(),
            "Each run uses a new complete copy of the captured Git tree. Checkout filters, hooks, submodules, LFS and network fetch are not executed.".into(),
            format!("Environment is cleared: PATH={FIXED_PATH}, CI=1, LANG=C, LC_ALL=C; HOME and TMPDIR are isolated per run. Toolchains needing other environment or dependency installation may fail."),
            "Lockfiles are identified, but installed dependencies, transitive executables, SDK assembly and cross-repository integration remain unconfirmed. Passed means this command exited successfully against unchanged captured sources.".into(),
            "Unix process groups stop ordinary descendants. Commands that deliberately detach are outside this trusted adapter; after an application crash, interrupted processes may need manual cleanup.".into(),
            "Source verification checks captured files before and after execution. Generated outputs are not reused; transient modifications restored by a command are not observed. Host resource use and network access are not sandboxed.".into(),
        ],
    };
    save(&dir.join("plan.json"), &plan)?;
    save(
        &dir.join("plan.integrity.json"),
        &file_hash(&dir.join("plan.json"))?,
    )?;
    Ok(plan)
}
pub fn load_plan(store: &Path, id: &str) -> Result<ValidationPlan> {
    valid_id(id)?;
    real_directory(&store.join("plans"))?;
    real_directory(&store.join("plans").join(id))?;
    let plan: ValidationPlan = read(&store.join("plans").join(id).join("plan.json"))?;
    let integrity: String = read(&store.join("plans").join(id).join("plan.integrity.json"))?;
    if file_hash(&store.join("plans").join(id).join("plan.json"))? != integrity {
        return Err(invalid("Stored plan identity changed"));
    }
    if plan.id != id
        || !relative(&plan.working_directory, true)
        || !(1..=3600).contains(&plan.timeout_seconds)
        || plan.args.len() > 256
        || plan.args.iter().map(|s| s.len()).sum::<usize>() > 65536
    {
        return Err(invalid("Invalid stored plan"));
    }
    let expected = digest(
        &serde_json::to_vec(&(
            &plan.program,
            &plan.program_id,
            &plan.args,
            &plan.working_directory,
            plan.timeout_seconds,
            FIXED_PATH,
            "isolated-home-tmp-ci-c-v1",
        ))
        .map_err(io)?,
    );
    if expected != plan.command_id {
        return Err(invalid("Stored command configuration changed"));
    }
    let dir = fs::canonicalize(store).map_err(io)?.join("plans").join(id);
    for (snapshot, name) in std::iter::once((&plan.candidate, "candidate"))
        .chain(plan.baseline.as_ref().map(|s| (s, "baseline")))
    {
        if Path::new(&snapshot.root) != dir.join(name)
            || snapshot.file_count as usize > MAX_FILES
            || snapshot.total_bytes as usize > MAX_BYTES
        {
            return Err(invalid("Invalid stored snapshot path or bounds"));
        }
    }
    Ok(plan)
}
pub fn load_result(store: &Path, id: &str) -> Result<ValidationResult> {
    valid_id(id)?;
    real_directory(&store.join("runs"))?;
    real_directory(&store.join("runs").join(id))?;
    read(&store.join("runs").join(id).join("result.json"))
}
fn verify(snapshot: &ValidationSnapshot, cancel: &AtomicBool) -> Result<Vec<String>> {
    let root = Path::new(&snapshot.root);
    let manifest: Manifest = read(&root.with_extension("manifest.json"))?;
    if digest(&serde_json::to_vec(&manifest).map_err(io)?) != snapshot.content_id {
        return Err(invalid("Frozen source manifest was modified"));
    }
    if manifest.entries.len() > MAX_FILES
        || manifest.entries.iter().any(|e| e.size > MAX_FILE)
        || manifest.entries.iter().map(|e| e.size).sum::<usize>() > MAX_BYTES
    {
        return Err(invalid("Manifest exceeds budget"));
    }
    if fs::symlink_metadata(root).map_or(true, |m| !m.is_dir() || m.file_type().is_symlink()) {
        return Err(invalid("Candidate root is not a real directory"));
    }
    let mut changes = Vec::new();
    for e in manifest.entries {
        check(cancel)?;
        if !relative(&e.path, false) {
            return Err(invalid("Unsafe manifest path"));
        }
        let p = root.join(&e.path);
        let mut cursor = root.to_path_buf();
        let mut safe = true;
        for c in Path::new(&e.path).components() {
            cursor.push(c);
            if fs::symlink_metadata(&cursor).map_or(true, |m| m.file_type().is_symlink()) {
                safe = false;
                break;
            }
        }
        let same = safe
            && fs::metadata(&p).is_ok_and(|m| m.is_file() && m.len() == e.size as u64)
            && file_hash(&p).is_ok_and(|d| d == e.digest);
        #[cfg(unix)]
        let same = same && {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(&p).is_ok_and(|m| (m.permissions().mode() & 0o111 != 0) == e.executable)
        };
        if !same {
            changes.push(e.path);
        }
    }
    Ok(changes)
}
fn copy_snapshot(
    snapshot: &ValidationSnapshot,
    root: &Path,
    cancel: &AtomicBool,
) -> Result<ValidationSnapshot> {
    let manifest: Manifest = read(&Path::new(&snapshot.root).with_extension("manifest.json"))?;
    fs::create_dir(root).map_err(io)?;
    for e in &manifest.entries {
        check(cancel)?;
        let dest = root.join(&e.path);
        fs::create_dir_all(dest.parent().unwrap()).map_err(io)?;
        fs::copy(Path::new(&snapshot.root).join(&e.path), dest).map_err(io)?;
    }
    save(&root.with_extension("manifest.json"), &manifest)?;
    let mut copy = snapshot.clone();
    copy.root = root.to_string_lossy().into();
    Ok(copy)
}
struct PlanLock(File);
impl Drop for PlanLock {
    fn drop(&mut self) {
        // A concurrent fork may briefly retain this open file description even
        // with CLOEXEC. Closing our fd alone would then leave flock held until
        // that child execs. Release the shared lock explicitly before closing.
        let _ = self.0.unlock();
    }
}
fn lock_plan(store: &Path, id: &str) -> Result<PlanLock> {
    valid_id(id)?;
    real_directory(&store.join("plans"))?;
    real_directory(&store.join("plans").join(id))?;
    let path = store.join("plans").join(id).join("running.lock");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(io)?;
    file.try_lock().map_err(io)?;
    Ok(PlanLock(file))
}
/// Run only a previously prepared, explicitly approved plan. Callbacks are ordered, bounded logs.
pub fn run(
    store: &Path,
    plan_id: &str,
    target: ValidationTarget,
    cancel: &AtomicBool,
    mut emit: impl FnMut(&ValidationLog),
) -> Result<ValidationResult> {
    let canonical_store = fs::canonicalize(store).map_err(io)?;
    let store = canonical_store.as_path();
    real_directory(&store.join("runs"))?;
    let plan = load_plan(store, plan_id)?;
    let snapshot = match target {
        ValidationTarget::Candidate => &plan.candidate,
        ValidationTarget::Baseline => plan
            .baseline
            .as_ref()
            .ok_or_else(|| invalid("Plan has no baseline"))?,
    };
    let _lock = lock_plan(store, plan_id)?;
    let id = Uuid::new_v4().to_string();
    let dir = store.join("runs").join(&id);
    fs::create_dir(&dir).map_err(io)?;
    let mut result = ValidationResult {
        id,
        plan_id: plan_id.into(),
        target,
        status: ValidationStatus::Running,
        exit_code: None,
        started_at_ms: now(),
        finished_at_ms: 0.0,
        source_id: snapshot.content_id.clone(),
        program_id: plan.program_id.clone(),
        command_id: plan.command_id.clone(),
        dependency_status: plan.dependency_status.clone(),
        source_changed: false,
        changed_paths: vec![],
        log_tail: vec![],
        log_truncated: false,
        message: String::new(),
    };
    save(&dir.join("result.json"), &result)?;
    let execution = (|| -> Result<()> {
        if !plan.executable || !snapshot.complete {
            result.status = ValidationStatus::NotRun;
            result.message = plan.blockers.join("; ");
            return Ok(());
        }
        check(cancel)?;
        if file_hash(Path::new(&plan.program))? != plan.program_id {
            result.status = ValidationStatus::NotRun;
            result.message = "Executable identity changed after plan creation".into();
            return Ok(());
        }
        let changed = verify(snapshot, cancel)?;
        if !changed.is_empty() {
            result.status = ValidationStatus::NotRun;
            result.changed_paths = changed;
            result.source_changed = true;
            result.message = "Frozen source changed after plan creation".into();
            return Ok(());
        }
        let source = copy_snapshot(snapshot, &dir.join("source"), cancel)?;
        check(cancel)?;
        if !verify(&source, cancel)?.is_empty() {
            return Err(invalid("Source copy identity mismatch"));
        }
        let home = dir.join("home");
        let tmp = dir.join("tmp");
        fs::create_dir(&home).map_err(io)?;
        fs::create_dir(&tmp).map_err(io)?;
        let mut command = Command::new(&plan.program);
        command
            .args(&plan.args)
            .current_dir(Path::new(&source.root).join(&plan.working_directory))
            .env_clear()
            .env("PATH", FIXED_PATH)
            .env("HOME", &home)
            .env("TMPDIR", &tmp)
            .env("CI", "1")
            .env("LANG", "C")
            .env("LC_ALL", "C");
        let mut logs = File::create(dir.join("logs.jsonl")).map_err(io)?;
        let mut tail = VecDeque::new();
        let mut tail_bytes = 0;
        let mut accepted = 0;
        let mut sequence = 0;
        let mut log_error = None;
        let outcome = process(
            &mut command,
            cancel,
            Instant::now() + Duration::from_secs(plan.timeout_seconds as u64),
            |stream, bytes| {
                if accepted >= MAX_LOG || sequence >= 4096 {
                    result.log_truncated = true;
                    return;
                }
                let take = bytes.len().min(MAX_LOG - accepted);
                if take < bytes.len() {
                    result.log_truncated = true;
                }
                sequence += 1;
                let entry = ValidationLog {
                    sequence,
                    stream: stream.into(),
                    text: String::from_utf8_lossy(&bytes[..take]).into(),
                };
                let encoded = match serde_json::to_vec(&entry) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        log_error = Some(e.to_string());
                        return;
                    }
                };
                if accepted + encoded.len() + 1 > MAX_LOG {
                    result.log_truncated = true;
                    sequence -= 1;
                    accepted = MAX_LOG;
                    return;
                }
                accepted += encoded.len() + 1;
                if let Err(e) = logs.write_all(&encoded).and_then(|_| logs.write_all(b"\n")) {
                    log_error = Some(e.to_string());
                }
                emit(&entry);
                tail_bytes += entry.text.len();
                tail.push_back(entry);
                while tail_bytes > TAIL || tail.len() > 256 {
                    if let Some(e) = tail.pop_front() {
                        tail_bytes -= e.text.len();
                        result.log_truncated = true;
                    }
                }
            },
        );
        result.log_tail = tail.into();
        logs.sync_all().map_err(io)?;
        let outcome = outcome.map_err(io)?;
        result.exit_code = outcome.code;
        result.status = if outcome.cancelled {
            ValidationStatus::Cancelled
        } else if outcome.timed_out {
            ValidationStatus::Indeterminate
        } else if outcome.code == Some(0) {
            ValidationStatus::Passed
        } else {
            ValidationStatus::Failed
        };
        result.message = if outcome.cancelled {
            "Cancelled; process group terminated"
        } else if outcome.timed_out {
            "Timeout; process group terminated; validation is indeterminate"
        } else if outcome.code == Some(0) {
            "Command passed; dependencies and cross-repository integration remain unconfirmed"
        } else {
            "Command failed"
        }
        .into();
        result.changed_paths = match verify(&source, &AtomicBool::new(false)) {
            Ok(paths) => paths,
            Err(_) => vec!["<source or manifest could not be verified>".into()],
        };
        result.source_changed = !result.changed_paths.is_empty();
        if result.source_changed
            || file_hash(Path::new(&plan.program)).map_or(true, |hash| hash != plan.program_id)
            || log_error.is_some()
        {
            result.status = ValidationStatus::Indeterminate;
            result.message =
                "Captured source, executable, or persisted logs changed during execution".into();
        }
        Ok(())
    })();
    if let Err(error) = execution {
        result.status = if matches!(error, ValidationError::Cancelled { .. }) {
            ValidationStatus::Cancelled
        } else {
            ValidationStatus::EnvError
        };
        result.message = format!("{error:?}");
    }
    result.finished_at_ms = now();
    save(&dir.join("result.json"), &result)?;
    Ok(result)
}
pub fn history(store: &Path, limit: u32) -> Result<Vec<ValidationResult>> {
    let dir = store.join("runs");
    if !dir.exists() {
        return Ok(vec![]);
    }
    real_directory(&dir)?;
    let mut results = Vec::new();
    for entry in fs::read_dir(dir).map_err(io)?.take(10000) {
        let entry = entry.map_err(io)?;
        if real_directory(&entry.path()).is_err() {
            continue;
        }
        if let Ok(mut result) = read::<ValidationResult>(&entry.path().join("result.json")) {
            result.log_tail.clear();
            results.push(result);
        }
    }
    results.sort_by(|a, b| b.started_at_ms.total_cmp(&a.started_at_ms));
    results.truncate(limit.min(100) as usize);
    Ok(results)
}
/// Startup only: never call while this application's jobs are still running.
pub fn recover_interrupted(store: &Path) -> Result<u32> {
    let mut count = 0;
    let runs = store.join("runs");
    if !runs.exists() {
        return Ok(0);
    }
    real_directory(&runs)?;
    for entry in fs::read_dir(runs).map_err(io)?.take(10000) {
        let entry = entry.map_err(io)?;
        if real_directory(&entry.path()).is_err() {
            continue;
        }
        let Ok(mut result) = read::<ValidationResult>(&entry.path().join("result.json")) else {
            continue;
        };
        valid_id(&result.id)?;
        valid_id(&result.plan_id)?;
        if entry.file_name().to_str() != Some(&result.id) {
            return Err(invalid("Run identity does not match directory"));
        }
        real_directory(&store.join("plans"))?;
        real_directory(&store.join("plans").join(&result.plan_id))?;
        if result.status == ValidationStatus::Running {
            let Ok(_lock) = lock_plan(store, &result.plan_id) else {
                continue;
            };
            result.status = ValidationStatus::Interrupted;
            result.finished_at_ms = now();
            if let Ok(file) = File::open(entry.path().join("logs.jsonl")) {
                let mut bytes = Vec::new();
                if file.take(MAX_LOG as u64).read_to_end(&mut bytes).is_ok() {
                    let mut tail = VecDeque::new();
                    let mut size = 0;
                    for line in bytes.split(|b| *b == b'\n') {
                        if let Ok(log) = serde_json::from_slice::<ValidationLog>(line) {
                            size += log.text.len();
                            tail.push_back(log);
                            while size > TAIL || tail.len() > 256 {
                                if let Some(log) = tail.pop_front() {
                                    size -= log.text.len();
                                    result.log_truncated = true;
                                }
                            }
                        }
                    }
                    result.log_tail = tail.into();
                }
            }
            result.message="Application stopped before a terminal result was persisted; no success is inferred and old PIDs are not reused for cleanup".into();
            save(
                &store.join("runs").join(&result.id).join("result.json"),
                &result,
            )?;
            count += 1;
        }
    }
    Ok(count)
}
struct Outcome {
    code: Option<i32>,
    cancelled: bool,
    timed_out: bool,
}
#[cfg(unix)]
fn process(
    command: &mut Command,
    cancel: &AtomicBool,
    deadline: Instant,
    mut output: impl FnMut(&str, &[u8]),
) -> std::io::Result<Outcome> {
    use nix::{
        fcntl::{fcntl, FcntlArg, OFlag},
        sys::signal::{killpg, Signal},
        unistd::Pid,
    };
    use std::{os::unix::process::CommandExt, process::Stdio};
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let child = command.spawn()?;
    let pid = Pid::from_raw(child.id() as i32);
    struct Group {
        pid: Pid,
        child: std::process::Child,
    }
    impl Drop for Group {
        fn drop(&mut self) {
            let _ = killpg(self.pid, Signal::SIGKILL);
            let _ = self.child.wait();
        }
    }
    let mut group = Group { pid, child };
    let mut stdout = group.child.stdout.take().unwrap();
    let mut stderr = group.child.stderr.take().unwrap();
    for fd in [
        &stdout as &dyn std::os::fd::AsFd,
        &stderr as &dyn std::os::fd::AsFd,
    ] {
        let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(std::io::Error::other)?;
        fcntl(
            fd,
            FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
        )
        .map_err(std::io::Error::other)?;
    }
    let mut cancelled = false;
    let mut timed_out = false;
    let mut killed = None;
    let mut status = None;
    loop {
        for (name, reader) in [
            ("stdout", &mut stdout as &mut dyn Read),
            ("stderr", &mut stderr as &mut dyn Read),
        ] {
            let mut buf = [0; 8192];
            for _ in 0..16 {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => output(name, &buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
        }
        if status.is_none() {
            status = group.child.try_wait()?;
        }
        if killed.is_none()
            && (status.is_some() || cancel.load(Ordering::Relaxed) || Instant::now() >= deadline)
        {
            cancelled = cancel.load(Ordering::Relaxed);
            timed_out = status.is_none() && Instant::now() >= deadline;
            let _ = killpg(pid, Signal::SIGTERM);
            killed = Some(Instant::now());
        }
        if killed.is_some_and(|time| {
            time.elapsed() >= Duration::from_millis(if cancelled || timed_out { 150 } else { 10 })
        }) {
            let _ = killpg(pid, Signal::SIGKILL);
            if status.is_none() {
                status = Some(group.child.wait()?);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(Outcome {
        code: status.and_then(|s| s.code()),
        cancelled,
        timed_out,
    })
}
#[cfg(not(unix))]
fn process(
    _command: &mut Command,
    _cancel: &AtomicBool,
    _deadline: Instant,
    _output: impl FnMut(&str, &[u8]),
) -> std::io::Result<Outcome> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Reliable process tree termination unavailable on this platform",
    ))
}

#[cfg(all(test, unix))]
mod lock_tests {
    use super::*;

    struct Fixture(PathBuf, String);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("diffkin-plan-lock-test-{}", Uuid::new_v4()));
            let id = Uuid::new_v4().to_string();
            fs::create_dir_all(root.join("plans").join(&id)).unwrap();
            Self(root, id)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn dropping_plan_lock_releases_it_while_an_inherited_fd_remains_open() {
        let fixture = Fixture::new();
        let guard = lock_plan(&fixture.0, &fixture.1).unwrap();
        // dup and fork reference the same open file description and flock.
        let inherited = guard.0.try_clone().unwrap();
        assert!(lock_plan(&fixture.0, &fixture.1).is_err());
        drop(guard);
        assert!(lock_plan(&fixture.0, &fixture.1).is_ok());
        drop(inherited);
    }

    #[test]
    fn panic_releases_plan_lock_while_an_inherited_fd_remains_open() {
        let fixture = Fixture::new();
        let guard = lock_plan(&fixture.0, &fixture.1).unwrap();
        let inherited = guard.0.try_clone().unwrap();
        let result = std::panic::catch_unwind(move || {
            let _guard = guard;
            panic!("controlled plan failure");
        });
        assert!(result.is_err());
        assert!(lock_plan(&fixture.0, &fixture.1).is_ok());
        drop(inherited);
    }
}
