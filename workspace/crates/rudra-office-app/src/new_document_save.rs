//! Safe one-shot Save As for newly created documents.
//!
//! Imported documents are deliberately outside this contract. The destination
//! must not exist, so neither a source file nor an unrelated user file can be
//! overwritten while Office export fidelity is still being accepted.

use std::{fs::OpenOptions, io::Write, path::Path};

pub(crate) fn write_new_document(
    path: &Path,
    expected_extension: &str,
    bytes: &[u8],
) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("Экспорт вернул пустой файл; документ не сохранён.".to_owned());
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected_extension))
    {
        return Err(format!(
            "Имя файла должно заканчиваться на .{expected_extension}."
        ));
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            format!(
                "Не удалось создать {}. Выберите новое имя файла: {error}",
                path.display()
            )
        })?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        // The exact file was created above with create_new by this function.
        let cleanup = std::fs::remove_file(path);
        return Err(match cleanup {
            Ok(()) => format!("Не удалось записать документ; неполный файл удалён: {error}"),
            Err(cleanup_error) => format!(
                "Не удалось записать документ: {error}. Удалите неполный файл {} вручную: {cleanup_error}",
                path.display()
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_path(extension: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("current time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rudra-office-save-test-{}-{nonce}.{extension}",
            std::process::id()
        ))
    }

    #[test]
    fn rejects_wrong_extension_before_creating_a_file() {
        let path = unique_path("invalid");
        assert!(write_new_document(&path, "docx", b"test").is_err());
        assert!(!path.exists());
    }

    #[test]
    fn rejects_empty_bytes_before_creating_a_file() {
        let path = unique_path("docx");
        assert!(write_new_document(&path, "docx", b"").is_err());
        assert!(!path.exists());
    }
}
