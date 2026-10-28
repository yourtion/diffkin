use diffkin_domain::{CompareMode, CompareRequest, ComparisonSpec};
use std::{env, ffi::OsStr, fs::File, io::Read, process::ExitCode, sync::atomic::AtomicBool};

const HELP: &str = "Diffkin — read-only code comparison\n\nUsage:\n  diffkin <before-file> <after-file>\n  diffkin --structure <before-file> <after-file>\n  diffkin --repo <directory> [--mode worktree|unstaged|staged|commits]\n          [--base <revision>] [--head <revision>] [--untracked]\n\nRepository output includes immutable source identities, metadata and structural analysis.\nCommit mode defaults to HEAD~1 versus HEAD. Worktree mode defaults to HEAD versus raw files.\nNo Git filters, checkout, hooks, external diffs or object fetching are performed.\nExit codes: 0 identical, 1 different, 2 error. JSON is written to stdout.\n";

fn read_input(path: &OsStr) -> Result<String, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(diffkin_application::MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > diffkin_application::MAX_INPUT_BYTES {
        return Err("Input exceeds the 200 KB detailed-analysis limit; use repository mode for bounded large-file previews".into());
    }
    String::from_utf8(bytes).map_err(|_| "Input must be valid UTF-8".into())
}

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn repository(args: &[std::ffi::OsString]) -> Result<ExitCode, String> {
    let mut spec = ComparisonSpec {
        repository: String::new(),
        mode: CompareMode::Worktree,
        base: None,
        head: None,
        include_untracked: false,
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].to_str().ok_or("Options must be UTF-8")?;
        if !seen.insert(flag) {
            return Err(format!("Duplicate option: {flag}"));
        }
        if flag == "--untracked" {
            spec.include_untracked = true;
            index += 1;
            continue;
        }
        if !matches!(flag, "--repo" | "--mode" | "--base" | "--head") {
            return Err(format!("Unknown option: {flag}"));
        }
        let value = args
            .get(index + 1)
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("Missing UTF-8 value for {flag}"))?;
        match flag {
            "--repo" => spec.repository = value.into(),
            "--mode" => {
                spec.mode = match value {
                    "commits" => CompareMode::Commits,
                    "staged" => CompareMode::Staged,
                    "unstaged" => CompareMode::Unstaged,
                    "worktree" => CompareMode::Worktree,
                    _ => return Err(format!("Unknown comparison mode: {value}")),
                }
            }
            "--base" => spec.base = Some(value.into()),
            "--head" => spec.head = Some(value.into()),
            _ => unreachable!(),
        }
        index += 2;
    }
    if spec.repository.is_empty() {
        return Err("--repo requires a directory".into());
    }
    if spec.mode != CompareMode::Commits && (spec.base.is_some() || spec.head.is_some()) {
        return Err("--base and --head require --mode commits".into());
    }
    if spec.include_untracked && matches!(spec.mode, CompareMode::Commits | CompareMode::Staged) {
        return Err("--untracked requires worktree or unstaged mode".into());
    }
    let cancel = AtomicBool::new(false);
    let comparison =
        diffkin_application::capture(&spec, &cancel).map_err(|error| error.to_string())?;
    let manifest = comparison.manifest(0, usize::MAX);
    let mut files = Vec::with_capacity(manifest.files.len());
    for summary in &manifest.files {
        let file = comparison
            .read_file(&summary.id)
            .map_err(|error| error.to_string())?;
        files.push(
            diffkin_application::analyze_file(file, &cancel).map_err(|error| error.to_string())?,
        );
    }
    let different = manifest.total_files > 0;
    print_json(
        &serde_json::json!({ "schemaVersion": diffkin_domain::SCHEMA_VERSION, "manifest": manifest, "files": files }),
    )?;
    Ok(ExitCode::from(u8::from(different)))
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("diffkin {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        print!("{HELP}");
        return Ok(ExitCode::SUCCESS);
    }
    if args.iter().any(|argument| argument == "--repo") {
        return repository(&args);
    }
    let structured = args
        .first()
        .is_some_and(|argument| argument == "--structure");
    let paths = if structured { &args[1..] } else { &args[..] };
    if paths.len() != 2 {
        return Err(HELP.into());
    }
    let request = CompareRequest {
        before: read_input(&paths[0]).map_err(|error| format!("Before file: {error}"))?,
        after: read_input(&paths[1]).map_err(|error| format!("After file: {error}"))?,
    };
    let different = request.before != request.after;
    if structured {
        let result = diffkin_application::analyze_text(
            &paths[1].to_string_lossy(),
            &request.before,
            &request.after,
            &AtomicBool::new(false),
        )
        .map_err(|error| {
            serde_json::to_string(&error).unwrap_or_else(|_| "Comparison failed".into())
        })?;
        print_json(&result)?;
    } else {
        let result = diffkin_application::compare_text(&request).map_err(|error| {
            serde_json::to_string(&error).unwrap_or_else(|_| "Comparison failed".into())
        })?;
        print_json(&result)?;
    }
    Ok(ExitCode::from(u8::from(different)))
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
