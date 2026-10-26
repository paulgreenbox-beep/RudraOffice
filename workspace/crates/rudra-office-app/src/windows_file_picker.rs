use std::{ffi::OsString, iter, mem::size_of, os::windows::ffi::OsStringExt, path::PathBuf};

use windows_uia::{
    Win32::UI::{
        Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER,
            OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
            OFN_PATHMUSTEXIST, OPEN_FILENAME_FLAGS, OPENFILENAMEW,
        },
        WindowsAndMessaging::GetForegroundWindow,
    },
    core::{PCWSTR, PWSTR},
};

const FILE_NAME_CAPACITY: usize = 32_768;

fn nul_terminated(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(iter::once(0)).collect()
}

fn office_filter() -> Vec<u16> {
    "Документы Office (*.docx;*.xlsx;*.pdf;*.pptx)\0*.docx;*.xlsx;*.pdf;*.pptx\0Все файлы (*.*)\0*.*\0\0"
        .encode_utf16()
        .collect()
}

fn save_format(extension: &str) -> Result<(&'static str, &'static str), String> {
    match extension {
        "docx" => Ok(("Документ Word", "docx")),
        "xlsx" => Ok(("Таблица Excel", "xlsx")),
        "pptx" => Ok(("Презентация PowerPoint", "pptx")),
        "pdf" => Ok(("Документ PDF", "pdf")),
        _ => Err(format!("Неподдерживаемый формат сохранения: {extension}")),
    }
}

fn save_filter(extension: &str) -> Result<Vec<u16>, String> {
    let (label, extension) = save_format(extension)?;
    Ok(format!("{label} (*.{extension})\0*.{extension}\0\0")
        .encode_utf16()
        .collect())
}

fn selected_path(buffer: &[u16]) -> Option<PathBuf> {
    let length = buffer.iter().position(|value| *value == 0)?;
    (length > 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..length])))
}

/// Opens the standard Windows Explorer-style file picker.
///
/// `Ok(None)` means that the user cancelled the dialog; cancellation is not
/// an application error and must not alter the active document session.
pub fn select_office_document() -> Result<Option<PathBuf>, String> {
    let mut file_name = vec![0_u16; FILE_NAME_CAPACITY];
    let filter = office_filter();
    let title = nul_terminated("Открыть документ");
    let flags = OPEN_FILENAME_FLAGS(
        OFN_EXPLORER.0
            | OFN_FILEMUSTEXIST.0
            | OFN_PATHMUSTEXIST.0
            | OFN_NOCHANGEDIR.0
            | OFN_HIDEREADONLY.0,
    );
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        // SAFETY: this only reads the foreground HWND and does not transfer ownership.
        hwndOwner: unsafe { GetForegroundWindow() },
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(file_name.as_mut_ptr()),
        nMaxFile: file_name.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: flags,
        ..Default::default()
    };

    // SAFETY: all pointers in `dialog` refer to live UTF-16 buffers for the
    // duration of this blocking Win32 call, and `nMaxFile` matches the buffer.
    if unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        selected_path(&file_name)
            .map(Some)
            .ok_or_else(|| "Windows file dialog returned an empty path.".to_owned())
    } else {
        // A zero extended error is the documented signal for user cancellation.
        let error = unsafe { CommDlgExtendedError() }.0;
        if error == 0 {
            Ok(None)
        } else {
            Err(format!(
                "Windows file dialog failed with common-dialog error 0x{error:08X}."
            ))
        }
    }
}

/// Selects a destination for a newly created document. Callers must still
/// reject an existing target before writing: the dialog confirmation alone
/// is not a safe overwrite contract for this preview application.
pub fn select_save_document(extension: &str) -> Result<Option<PathBuf>, String> {
    let (label, extension) = save_format(extension)?;
    let filter = save_filter(extension)?;
    let title = nul_terminated(&format!("Сохранить {label} как"));
    let default_extension = nul_terminated(extension);
    let initial_name = nul_terminated(&format!("Новый документ.{extension}"));
    let mut file_name = vec![0_u16; FILE_NAME_CAPACITY];
    file_name[..initial_name.len()].copy_from_slice(&initial_name);
    let flags = OPEN_FILENAME_FLAGS(
        OFN_EXPLORER.0 | OFN_PATHMUSTEXIST.0 | OFN_NOCHANGEDIR.0 | OFN_OVERWRITEPROMPT.0,
    );
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        // SAFETY: the foreground HWND is only borrowed for the modal dialog.
        hwndOwner: unsafe { GetForegroundWindow() },
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(file_name.as_mut_ptr()),
        nMaxFile: file_name.len() as u32,
        lpstrDefExt: PCWSTR(default_extension.as_ptr()),
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: flags,
        ..Default::default()
    };

    // SAFETY: dialog pointers refer to live UTF-16 buffers for this call.
    if unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
        let path = selected_path(&file_name)
            .ok_or_else(|| "Windows save dialog returned an empty path.".to_owned())?;
        let selected_extension = path.extension().and_then(|value| value.to_str());
        if !selected_extension.is_some_and(|value| value.eq_ignore_ascii_case(extension)) {
            return Err(format!(
                "Выберите имя файла с расширением .{extension}; выбранный путь не сохранён."
            ));
        }
        Ok(Some(path))
    } else {
        let error = unsafe { CommDlgExtendedError() }.0;
        if error == 0 {
            Ok(None)
        } else {
            Err(format!(
                "Windows save dialog failed with common-dialog error 0x{error:08X}."
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_filter_is_double_nul_terminated_and_lists_supported_formats() {
        let filter = office_filter();
        assert_eq!(filter.last(), Some(&0));
        assert_eq!(filter.get(filter.len().saturating_sub(2)), Some(&0));
        let decoded = String::from_utf16_lossy(&filter);
        for extension in ["*.docx", "*.xlsx", "*.pdf", "*.pptx"] {
            assert!(decoded.contains(extension));
        }
    }

    #[test]
    fn selected_path_stops_at_first_nul() {
        let source = nul_terminated(r"C:\Temp\пример.docx");
        assert_eq!(
            selected_path(&source),
            Some(PathBuf::from(r"C:\Temp\пример.docx"))
        );
    }

    #[test]
    fn save_filter_is_bounded_to_supported_extension() {
        let filter = save_filter("docx").unwrap();
        assert_eq!(filter.last(), Some(&0));
        assert_eq!(filter.get(filter.len() - 2), Some(&0));
        let decoded = String::from_utf16_lossy(&filter);
        assert!(decoded.contains("*.docx"));
        assert!(!decoded.contains("*.xlsx"));
        assert!(save_filter("exe").is_err());
    }
}
