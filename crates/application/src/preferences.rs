//! Bounded, versioned UI preferences. No repository or command execution is triggered here.
use diffkin_domain::{AppPreferences, PreferencesError};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: usize = 64 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
type Result<T> = std::result::Result<T, PreferencesError>;

fn io(error: impl std::fmt::Display) -> PreferencesError {
    PreferencesError::Io {
        message: error.to_string(),
    }
}
fn invalid(message: impl Into<String>) -> PreferencesError {
    PreferencesError::Invalid {
        message: message.into(),
    }
}
fn corrupt(message: impl Into<String>) -> PreferencesError {
    PreferencesError::Corrupt {
        message: message.into(),
    }
}
fn unsafe_path(message: impl Into<String>) -> PreferencesError {
    PreferencesError::UnsafePath {
        message: message.into(),
    }
}
fn validate(preferences: &AppPreferences) -> Result<()> {
    if preferences.schema_version != 1 {
        return Err(PreferencesError::UnsupportedVersion {
            message: format!(
                "Preferences schema {} is unsupported; expected 1",
                preferences.schema_version
            ),
        });
    }
    let text = |value: &str, limit: usize| value.len() <= limit && !value.contains('\0');
    if preferences.recent.len() > 8
        || !text(&preferences.spec.repository, 4096)
        || preferences.recent.iter().any(|path| !text(path, 4096))
        || [&preferences.spec.base, &preferences.spec.head]
            .into_iter()
            .flatten()
            .any(|reference| !text(reference, 1024))
    {
        return Err(invalid(
            "Preferences exceed path/ref limits, contain a NUL character, or list more than 8 recent repositories",
        ));
    }
    Ok(())
}

/// The caller supplies the application data directory; preferences never choose this path.
pub struct PreferencesStore {
    directory: PathBuf,
}
impl PreferencesStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn directory(&self, create: bool) -> Result<Option<PathBuf>> {
        if !self.directory.is_absolute() {
            return Err(unsafe_path("Preferences directory must be absolute"));
        }
        match fs::symlink_metadata(&self.directory) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err(unsafe_path("Preferences directory is not a real directory")),
            Err(e) if e.kind() == ErrorKind::NotFound && !create => return Ok(None),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&self.directory).map_err(io)?;
                let meta = fs::symlink_metadata(&self.directory).map_err(io)?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err(unsafe_path("Preferences directory changed during creation"));
                }
            }
            Err(e) => return Err(io(e)),
        }
        fs::canonicalize(&self.directory).map(Some).map_err(io)
    }

    pub fn load(&self) -> Result<Option<AppPreferences>> {
        let Some(directory) = self.directory(false)? else {
            return Ok(None);
        };
        let path = directory.join("settings.json");
        let Some(metadata) = regular_file(&path)? else {
            return Ok(None);
        };
        if metadata.len() > MAX_BYTES as u64 {
            return Err(corrupt(
                "Preferences file exceeds 64 KiB; existing file was preserved",
            ));
        }
        let file = File::open(&path).map_err(io)?;
        if !file.metadata().map_err(io)?.is_file() || regular_file(&path)?.is_none() {
            return Err(unsafe_path("Preferences file changed while opening"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() > MAX_BYTES {
            return Err(corrupt(
                "Preferences file exceeds 64 KiB; existing file was preserved",
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
            corrupt(format!(
                "Preferences JSON is damaged: {e}; existing file was preserved"
            ))
        })?;
        let version = value
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| corrupt("Preferences schemaVersion is missing or invalid"))?;
        if version != 1 {
            return Err(PreferencesError::UnsupportedVersion {
                message: format!(
                    "Preferences schema {version} is unsupported; existing file was preserved"
                ),
            });
        }
        let preferences: AppPreferences = serde_json::from_value(value)
            .map_err(|e| corrupt(format!("Preferences fields are invalid: {e}")))?;
        validate(&preferences)
            .map_err(|e| corrupt(format!("Invalid stored preferences: {e:?}")))?;
        Ok(Some(preferences))
    }

    /// An explicit save can replace a corrupt/unsupported regular file; a read never does so.
    pub fn save(&self, preferences: &AppPreferences) -> Result<()> {
        validate(preferences)?;
        let mut bytes = serde_json::to_vec_pretty(preferences).map_err(io)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_BYTES {
            return Err(invalid("Preferences exceed the 64 KiB storage limit"));
        }
        let directory = self.directory(true)?.expect("created directory");
        let path = directory.join("settings.json");
        regular_file(&path)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp = directory.join(format!(
            ".settings-{}-{stamp}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(io)?;
        let cleanup = Temporary(temp.clone());
        file.write_all(&bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        if self.directory(false)?.as_ref() != Some(&directory) {
            return Err(unsafe_path("Preferences directory changed while saving"));
        }
        regular_file(&path)?;
        fs::rename(&temp, &path).map_err(io)?;
        drop(cleanup);
        #[cfg(unix)]
        File::open(&directory)
            .and_then(|file| file.sync_all())
            .map_err(io)?;
        Ok(())
    }
}

fn regular_file(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(Some(meta)),
        Ok(_) => Err(unsafe_path(
            "Preferences must be a regular file, not a symbolic link or directory",
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(e)),
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
