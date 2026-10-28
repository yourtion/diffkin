//! Real backend DTO fixture for browser transport tests. All input repositories and
//! executed scripts are authored here and deleted before this process exits.
#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use diffkin_application::{analyze_file, WorkspaceStore};
    use diffkin_domain::*;
    use std::{
        collections::BTreeMap,
        fs::{self, File},
        io::{Read, Write},
        os::unix::fs::DirBuilderExt,
        path::{Path, PathBuf},
        process::Command,
        sync::atomic::AtomicBool,
    };

    struct Temporary(PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        let output = Command::new("git")
            .current_dir(repo)
            .env_clear()
            .env("PATH", "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("LC_ALL", "C")
            .args([
                "--no-pager",
                "--no-lazy-fetch",
                "-c",
                "core.hooksPath=",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.allow=never",
            ])
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "Controlled fixture Git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
        Ok(())
    }
    fn validation_error(error: ValidationError) -> std::io::Error {
        std::io::Error::other(format!("Controlled fixture validation failed: {error:?}"))
    }

    let mut entropy = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut entropy)?;
    let suffix: String = entropy.iter().map(|byte| format!("{byte:02x}")).collect();
    let path = std::env::temp_dir().join(format!("diffkin-workflow-fixture-{suffix}"));
    fs::DirBuilder::new().mode(0o700).create(&path)?;
    let temporary = Temporary(fs::canonicalize(path)?);
    let root = &temporary.0;
    let before = "function scale(value: number) {\n  if (value < 0) return 0;\n  return value + 1;\n}\nfunction label() {\n  return 'fixture';\n}\n";
    let after = "function label() {\n  return 'fixture';\n}\nfunction scale(amount: number) {\n  if (amount <= 0) return 0;\n  return amount + 2;\n}\n";
    let check_script = "#!/bin/sh\nset -eu\nprintf 'controlled workflow fixture validation\\n'\ntest -f src/shared.ts\ngrep -q 'return amount + 2;' src/shared.ts\n";
    let mut repositories = Vec::new();
    for (id, label) in [
        ("service", "Fixture service"),
        ("consumer", "Fixture consumer"),
        ("context", "Fixture unchanged context"),
    ] {
        let repo = root.join(id);
        fs::create_dir(&repo)?;
        git(&repo, &["init", "-q", "-b", "main"])?;
        git(
            &repo,
            &["config", "user.name", "Diffkin controlled fixture"],
        )?;
        git(&repo, &["config", "user.email", "fixture@example.invalid"])?;
        fs::create_dir(repo.join("src"))?;
        let initial = if id == "consumer" {
            before.replace("'fixture'", "'consumer fixture'")
        } else {
            before.into()
        };
        fs::write(repo.join("src/shared.ts"), initial)?;
        fs::write(repo.join("check.sh"), check_script)?;
        git(&repo, &["add", "src/shared.ts", "check.sh"])?;
        git(&repo, &["commit", "-qm", "Controlled fixture baseline"])?;
        if id != "context" {
            let changed = if id == "consumer" {
                after.replace("'fixture'", "'consumer fixture'")
            } else {
                after.into()
            };
            fs::write(repo.join("src/shared.ts"), changed)?;
            git(&repo, &["add", "src/shared.ts"])?;
        }
        git(
            &repo,
            &[
                "commit",
                "--allow-empty",
                "-qm",
                "Controlled fixture candidate",
            ],
        )?;
        repositories.push(WorkspaceRepository {
            id: id.into(),
            label: label.into(),
            spec: ComparisonSpec {
                repository: repo.to_string_lossy().into(),
                mode: CompareMode::Commits,
                base: Some("HEAD~1".into()),
                head: Some("HEAD".into()),
                include_untracked: false,
                verify_all: false,
            },
        });
    }
    let config = WorkspaceConfig {
        id: "workflow-fixture".into(),
        name: "Real backend workflow fixture".into(),
        repositories,
    };
    let cancel = AtomicBool::new(false);
    let store = WorkspaceStore::new(root.join("workspace-store"))?;
    store.capture(&config, &cancel)?;
    let frozen = store.restore(&config.id, None)?;
    let service = frozen.repository("service")?;
    let consumer = frozen.repository("consumer")?;
    let service_manifest = service.manifest(0, 100);
    let consumer_manifest = consumer.manifest(0, 100);
    assert_eq!(service_manifest.total_files, 1);
    assert_eq!(consumer_manifest.total_files, 1);
    assert_eq!(
        frozen.repository("context")?.manifest(0, 100).total_files,
        0
    );
    let from = frozen.basis("service", &service_manifest.files[0].id)?;
    let to = frozen.basis("consumer", &consumer_manifest.files[0].id)?;
    store.set_review(&config.id, from.clone(), ReviewState::Reviewed)?;
    store.add_relation(
        &config.id,
        ManualRelation {
            id: "service-consumer".into(),
            from,
            to,
            note: "Controlled fixture: service change needs consumer review".into(),
        },
    )?;
    // Reload persisted evidence before analyzing; no handcrafted FileAnalysis DTOs.
    let restored = store.restore(&config.id, None)?;
    let session = store.load(&config.id)?;
    let mut files: BTreeMap<String, Vec<FileAnalysis>> = BTreeMap::new();
    for repository in &config.repositories {
        let frozen = restored.repository(&repository.id)?;
        let manifest = frozen.manifest(0, 100);
        let analyses = manifest
            .files
            .iter()
            .map(|file| analyze_file(frozen.read_file(&file.id)?, &cancel))
            .collect::<Result<Vec<_>, _>>()?;
        files.insert(manifest.id, analyses);
    }
    let validation_store = root.join("validation-store");
    let plan = diffkin_validation::prepare(
        &ValidationSpec {
            repository: root.join("service").to_string_lossy().into(),
            candidate_ref: "HEAD".into(),
            baseline_ref: Some("HEAD~1".into()),
            program: "/bin/sh".into(),
            args: vec!["check.sh".into()],
            working_directory: ".".into(),
            timeout_seconds: 5,
        },
        &validation_store,
        &cancel,
    )
    .map_err(validation_error)?;
    assert!(plan.executable && plan.candidate.complete);
    let mut results = Vec::new();
    for target in [ValidationTarget::Candidate, ValidationTarget::Baseline] {
        results.push(
            diffkin_validation::run(&validation_store, &plan.id, target, &cancel, |_| {})
                .map_err(validation_error)?,
        );
    }
    assert_eq!(results[0].status, ValidationStatus::Passed);
    assert_eq!(results[1].status, ValidationStatus::Failed);
    assert_eq!(results[0].command_id, results[1].command_id);
    assert_ne!(results[0].source_id, results[1].source_id);
    let output = serde_json::json!({
        "session": session,
        "files": files,
        "plan": plan,
        "results": results,
    });
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &output)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    drop(temporary);
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    eprintln!("workflow_fixture requires the supported Unix validation adapter");
    std::process::exit(2);
}
