use diffkin_application::preferences::PreferencesStore;
use diffkin_domain::{
    AppPreferences, CompareMode, ComparisonSpec, DiffLayout, PreferencesError, ViewPreferences,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
                "diffkin-preferences-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
    }
    fn store(&self) -> PreferencesStore {
        PreferencesStore::new(self.0.join("app-data"))
    }
    fn file(&self) -> PathBuf {
        self.0.join("app-data/settings.json")
    }
    fn write(&self, bytes: &[u8]) {
        fs::create_dir_all(self.file().parent().unwrap()).unwrap();
        fs::write(self.file(), bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn preferences() -> AppPreferences {
    AppPreferences {
        schema_version: 1,
        spec: ComparisonSpec {
            repository: "/nonexistent/观变 repository".into(),
            mode: CompareMode::Commits,
            base: Some("main".into()),
            head: Some("feature/视图".into()),
            include_untracked: false,
            verify_all: false,
        },
        recent: vec![
            "/nonexistent/观变 repository".into(),
            "/nonexistent/another".into(),
        ],
        view: ViewPreferences {
            layout: DiffLayout::Unified,
            fold_formats: false,
            collapse_unchanged: true,
        },
    }
}

#[test]
fn missing_preferences_do_not_create_storage_or_open_repositories() {
    let f = Fixture::new();
    assert!(f.store().load().unwrap().is_none());
    assert!(!f.0.exists());
    let mut value = preferences();
    value.spec.repository = f.0.join("never-opened-repository").to_string_lossy().into();
    f.store().save(&value).unwrap();
    assert!(!PathBuf::from(&value.spec.repository).exists());
    let restored = f.store().load().unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(value).unwrap()
    );
}

#[test]
fn damaged_and_unknown_versions_are_preserved_until_explicit_replacement() {
    let f = Fixture::new();
    for bytes in [b"{ broken".as_slice(), b"{\"schemaVersion\":2}".as_slice()] {
        f.write(bytes);
        assert!(matches!(
            f.store().load(),
            Err(PreferencesError::Corrupt { .. } | PreferencesError::UnsupportedVersion { .. })
        ));
        assert_eq!(fs::read(f.file()).unwrap(), bytes);
        f.store().save(&preferences()).unwrap();
        assert!(f.store().load().unwrap().is_some());
    }
    f.write(b"{ broken");
    assert!(f.store().load().is_err());
    f.write(&serde_json::to_vec(&preferences()).unwrap());
    assert!(
        f.store().load().unwrap().is_some(),
        "Manual file repair must recover without restarting the store"
    );
}

#[test]
fn invalid_saves_leave_the_previous_valid_file_untouched() {
    let f = Fixture::new();
    f.store().save(&preferences()).unwrap();
    let before = fs::read(f.file()).unwrap();
    let mut values = Vec::new();
    let mut too_many = preferences();
    too_many.recent = vec!["path".into(); 9];
    values.push(too_many);
    let mut long_path = preferences();
    long_path.spec.repository = "x".repeat(4097);
    values.push(long_path);
    let mut long_ref = preferences();
    long_ref.spec.head = Some("x".repeat(1025));
    values.push(long_ref);
    let mut nul = preferences();
    nul.recent.push("invalid\0path".into());
    values.push(nul);
    let mut future = preferences();
    future.schema_version = 2;
    values.push(future);
    for value in values {
        assert!(f.store().save(&value).is_err());
        assert_eq!(fs::read(f.file()).unwrap(), before);
    }
}

#[test]
fn persisted_scope_rejects_unknown_fields_and_oversized_files() {
    let f = Fixture::new();
    let mut value = serde_json::to_value(preferences()).unwrap();
    value["approvedCommand"] = "never execute me".into();
    f.write(&serde_json::to_vec(&value).unwrap());
    assert!(matches!(
        f.store().load(),
        Err(PreferencesError::Corrupt { .. })
    ));
    f.write(&vec![b' '; 64 * 1024 + 1]);
    assert!(matches!(
        f.store().load(),
        Err(PreferencesError::Corrupt { .. })
    ));
    assert_eq!(fs::metadata(f.file()).unwrap().len(), 64 * 1024 + 1);
}

#[test]
fn non_file_destination_is_recoverable_without_removing_it() {
    let f = Fixture::new();
    fs::create_dir_all(f.file()).unwrap();
    assert!(matches!(
        f.store().load(),
        Err(PreferencesError::UnsafePath { .. })
    ));
    assert!(matches!(
        f.store().save(&preferences()),
        Err(PreferencesError::UnsafePath { .. })
    ));
    assert!(f.file().is_dir());
    fs::remove_dir(f.file()).unwrap();
    f.store().save(&preferences()).unwrap();
    assert!(f.store().load().unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn file_and_directory_links_are_rejected_and_targets_remain_untouched() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    fs::create_dir_all(&f.0).unwrap();
    let external = f.0.join("external.json");
    fs::write(&external, b"outside sentinel").unwrap();
    fs::create_dir(f.file().parent().unwrap()).unwrap();
    symlink(&external, f.file()).unwrap();
    assert!(matches!(
        f.store().load(),
        Err(PreferencesError::UnsafePath { .. })
    ));
    assert!(matches!(
        f.store().save(&preferences()),
        Err(PreferencesError::UnsafePath { .. })
    ));
    assert_eq!(fs::read(&external).unwrap(), b"outside sentinel");
    fs::remove_file(f.file()).unwrap();
    fs::remove_dir(f.file().parent().unwrap()).unwrap();
    let outside = f.0.join("outside-directory");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, f.file().parent().unwrap()).unwrap();
    assert!(matches!(
        f.store().load(),
        Err(PreferencesError::UnsafePath { .. })
    ));
    assert!(matches!(
        f.store().save(&preferences()),
        Err(PreferencesError::UnsafePath { .. })
    ));
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn settings_have_private_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.store().save(&preferences()).unwrap();
    assert_eq!(
        fs::metadata(f.file()).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn concurrent_saves_publish_only_complete_records_and_leave_no_temp_files() {
    let f = Fixture::new();
    f.store().save(&preferences()).unwrap();
    std::thread::scope(|scope| {
        for writer in 0..3 {
            let directory = f.file().parent().unwrap().to_path_buf();
            scope.spawn(move || {
                let store = PreferencesStore::new(directory);
                for revision in 0..8 {
                    let mut value = preferences();
                    value.spec.head = Some(format!("writer-{writer}-{revision}"));
                    store.save(&value).unwrap();
                    let result = store.load().unwrap().unwrap();
                    assert_eq!(result.schema_version, 1);
                    assert_eq!(result.view.layout, DiffLayout::Unified);
                }
            });
        }
    });
    assert_eq!(fs::read_dir(f.file().parent().unwrap()).unwrap().count(), 1);
}
