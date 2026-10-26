use std::fmt;
use std::fs;
use std::path::Path;

/// Safety reserve for one in-memory open operation. This is not a product
/// compatibility boundary; format-specific structural limits remain separate.
pub(crate) const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FileAdmissionPolicy {
    max_bytes: u64,
}

impl FileAdmissionPolicy {
    pub(crate) const fn new(max_bytes: u64) -> Self {
        Self { max_bytes }
    }

    const fn admit_size(self, actual_bytes: u64) -> Result<(), FileAdmissionError> {
        if actual_bytes > self.max_bytes {
            Err(FileAdmissionError::TooLarge {
                actual_bytes,
                max_bytes: self.max_bytes,
            })
        } else {
            Ok(())
        }
    }
}

impl Default for FileAdmissionPolicy {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_DOCUMENT_BYTES)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AdmittedFile {
    declared_size: u64,
    policy: FileAdmissionPolicy,
}

impl AdmittedFile {
    pub(crate) const fn declared_size(self) -> u64 {
        self.declared_size
    }
}

#[derive(Debug)]
pub(crate) enum FileAdmissionError {
    Metadata(std::io::Error),
    Read(std::io::Error),
    ActualLengthNotRepresentable,
    TooLarge { actual_bytes: u64, max_bytes: u64 },
}

impl fmt::Display for FileAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => {
                write!(formatter, "не удалось получить размер файла: {error}")
            }
            Self::Read(error) => write!(formatter, "не удалось прочитать файл: {error}"),
            Self::ActualLengthNotRepresentable => {
                formatter.write_str("размер файла не представим в поддерживаемом диапазоне")
            }
            Self::TooLarge {
                actual_bytes,
                max_bytes,
            } => write!(
                formatter,
                "размер файла {actual_bytes} байт превышает текущий resource budget {max_bytes} байт"
            ),
        }
    }
}

impl std::error::Error for FileAdmissionError {}

pub(crate) fn admit(path: &Path) -> Result<AdmittedFile, FileAdmissionError> {
    admit_with_policy(path, FileAdmissionPolicy::default())
}

pub(crate) fn read_after_admission(
    path: &Path,
    admission: AdmittedFile,
) -> Result<Vec<u8>, FileAdmissionError> {
    let bytes = fs::read(path).map_err(FileAdmissionError::Read)?;
    let actual_size =
        u64::try_from(bytes.len()).map_err(|_| FileAdmissionError::ActualLengthNotRepresentable)?;
    admission.policy.admit_size(actual_size)?;
    Ok(bytes)
}

#[cfg(test)]
fn read_admitted_with_policy(
    path: &Path,
    policy: FileAdmissionPolicy,
) -> Result<Vec<u8>, FileAdmissionError> {
    let admission = admit_with_policy(path, policy)?;
    read_after_admission(path, admission)
}

fn admit_with_policy(
    path: &Path,
    policy: FileAdmissionPolicy,
) -> Result<AdmittedFile, FileAdmissionError> {
    let declared_size = fs::metadata(path)
        .map_err(FileAdmissionError::Metadata)?
        .len();
    policy.admit_size(declared_size)?;
    Ok(AdmittedFile {
        declared_size,
        policy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    fn temporary_path(label: &str) -> std::path::PathBuf {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rudra-office-file-admission-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn remove_file(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn accepts_file_at_exact_limit() {
        let path = temporary_path("exact-limit");
        fs::write(&path, b"abc").expect("fixture must be writable");

        let bytes = read_admitted_with_policy(&path, FileAdmissionPolicy::new(3))
            .expect("file at limit must be admitted");
        remove_file(&path);

        assert_eq!(bytes, b"abc");
    }

    #[test]
    fn accepts_file_below_limit() {
        let path = temporary_path("below-limit");
        fs::write(&path, b"ab").expect("fixture must be writable");

        let bytes = read_admitted_with_policy(&path, FileAdmissionPolicy::new(3))
            .expect("file below limit must be admitted");
        remove_file(&path);

        assert_eq!(bytes, b"ab");
    }

    #[test]
    fn rejects_oversize_file_from_metadata() {
        let path = temporary_path("oversize");
        {
            let file = fs::File::create(&path).expect("fixture must be writable");
            file.set_len(4).expect("fixture size must be set");
        }

        let error = read_admitted_with_policy(&path, FileAdmissionPolicy::new(3))
            .expect_err("file over the configured resource budget must be rejected");
        remove_file(&path);

        let message = error.to_string();
        assert!(message.contains("превышает текущий resource budget"));
        assert!(matches!(
            error,
            FileAdmissionError::TooLarge {
                actual_bytes,
                max_bytes
            } if actual_bytes == 4
                && max_bytes == 3
        ));
    }

    #[test]
    fn default_policy_allows_documents_larger_than_legacy_16_mib_cutoff() {
        const LEGACY_16_MIB: u64 = 16 * 1024 * 1024;

        let policy = FileAdmissionPolicy::default();
        assert_eq!(DEFAULT_MAX_DOCUMENT_BYTES, 512 * 1024 * 1024);
        assert!(policy.admit_size(LEGACY_16_MIB + 1).is_ok());
        assert!(policy.admit_size(64 * 1024 * 1024).is_ok());
        assert!(matches!(
            policy.admit_size(DEFAULT_MAX_DOCUMENT_BYTES + 1),
            Err(FileAdmissionError::TooLarge {
                actual_bytes,
                max_bytes
            }) if actual_bytes == DEFAULT_MAX_DOCUMENT_BYTES + 1
                && max_bytes == DEFAULT_MAX_DOCUMENT_BYTES
        ));
    }
}
