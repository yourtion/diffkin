use diffkin_domain::CompareRequest;
use std::{env, ffi::OsStr, fs::File, io::Read, process::ExitCode};

fn read_input(path: &OsStr) -> Result<String, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(diffkin_application::MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > diffkin_application::MAX_INPUT_BYTES {
        return Err("Input exceeds the prototype's 200 KB limit".into());
    }
    String::from_utf8(bytes).map_err(|_| "Input must be valid UTF-8".into())
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("diffkin {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("Usage: diffkin <before-file> <after-file>\nOutputs JSON. Exit codes: 0 identical, 1 different, 2 error.\nUTF-8 text only; 200 KB / 5,000 lines per input.");
        return Ok(ExitCode::SUCCESS);
    }
    if args.len() != 2 {
        return Err("Usage: diffkin <before-file> <after-file>".into());
    }
    let request = CompareRequest {
        before: read_input(&args[0]).map_err(|error| format!("Before file: {error}"))?,
        after: read_input(&args[1]).map_err(|error| format!("After file: {error}"))?,
    };
    let result = diffkin_application::compare_text(&request).map_err(|error| {
        serde_json::to_string(&error).unwrap_or_else(|_| "Comparison failed".into())
    })?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?
    );
    Ok(ExitCode::from(u8::from(result.added + result.removed > 0)))
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
