//! Argument contract shared by the separate Office launcher and editor.
//!
//! This module deliberately uses only the standard library. Arguments remain
//! individual `OsString`s; in particular, paths are never converted into a
//! shell command string.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// A document format that the editor can create directly.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DocumentKind {
    Docx,
    Xlsx,
    Pptx,
    Pdf,
}

impl DocumentKind {
    /// Return the canonical, lowercase extension without a leading dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Pdf => "pdf",
        }
    }

    /// Parse a supported extension, accepting an optional leading dot and
    /// ASCII case differences.
    pub fn parse_extension(extension: &OsStr) -> Option<Self> {
        let extension = extension.to_str()?;
        let extension = extension.strip_prefix('.').unwrap_or(extension);

        if extension.eq_ignore_ascii_case("docx") {
            Some(Self::Docx)
        } else if extension.eq_ignore_ascii_case("xlsx") {
            Some(Self::Xlsx)
        } else if extension.eq_ignore_ascii_case("pptx") {
            Some(Self::Pptx)
        } else if extension.eq_ignore_ascii_case("pdf") {
            Some(Self::Pdf)
        } else {
            None
        }
    }
}

/// A launch action requested of the editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchRequest {
    New(DocumentKind),
    Open(PathBuf),
    OpenDialog,
}

impl LaunchRequest {
    /// Convert this request to editor arguments without shell quoting or
    /// Unicode conversion of paths.
    pub fn editor_args(&self) -> Vec<OsString> {
        match self {
            Self::New(kind) => vec![OsString::from("--new"), OsString::from(kind.extension())],
            Self::Open(path) => vec![OsString::from("--open"), path.as_os_str().to_os_string()],
            Self::OpenDialog => vec![OsString::from("--open-dialog")],
        }
    }
}

/// Parse launcher/editor arguments.
///
/// Unrecognized arguments are ignored so Makepad's own flags can coexist with
/// this contract. Each recognized option consumes exactly its following
/// argument when it has a value; this preserves paths that happen to begin
/// with a hyphen. More than one recognized launch option is rejected.
pub fn parse_editor_args<I>(args: I) -> Result<Option<LaunchRequest>, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let mut request: Option<LaunchRequest> = None;

    while let Some(arg) = args.next() {
        if arg == OsStr::new("--new") {
            let extension = args
                .next()
                .ok_or_else(|| "--new requires a document extension".to_owned())?;
            let kind = DocumentKind::parse_extension(&extension).ok_or_else(|| {
                format!("unsupported --new format: {}", extension.to_string_lossy())
            })?;
            set_single_request(&mut request, LaunchRequest::New(kind))?;
        } else if arg == OsStr::new("--open") {
            let path = args
                .next()
                .ok_or_else(|| "--open requires a path".to_owned())?;
            if path.is_empty() {
                return Err("--open requires a non-empty path".to_owned());
            }
            set_single_request(&mut request, LaunchRequest::Open(PathBuf::from(path)))?;
        } else if arg == OsStr::new("--open-dialog") {
            set_single_request(&mut request, LaunchRequest::OpenDialog)?;
        }
    }

    Ok(request)
}

fn set_single_request(
    current: &mut Option<LaunchRequest>,
    next: LaunchRequest,
) -> Result<(), String> {
    if current.is_some() {
        return Err("multiple or conflicting launch requests".to_owned());
    }
    *current = Some(next);
    Ok(())
}

/// Return the canonical editor executable beside `current_exe`.
pub fn sibling_editor_path(current_exe: &Path) -> PathBuf {
    #[cfg(windows)]
    let editor_name = "rudra-office-app.exe";
    #[cfg(not(windows))]
    let editor_name = "rudra-office-app";

    current_exe
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(editor_name)
}

#[cfg(test)]
mod tests {
    use super::{DocumentKind, LaunchRequest, parse_editor_args, sibling_editor_path};
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};

    fn strings(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn canonical_extensions_and_parsing_cover_all_document_kinds() {
        let cases = [
            (DocumentKind::Docx, "docx"),
            (DocumentKind::Xlsx, "xlsx"),
            (DocumentKind::Pptx, "pptx"),
            (DocumentKind::Pdf, "pdf"),
        ];

        for (kind, extension) in cases {
            assert_eq!(kind.extension(), extension);
            assert_eq!(
                DocumentKind::parse_extension(OsStr::new(extension)),
                Some(kind)
            );
            assert_eq!(
                DocumentKind::parse_extension(OsStr::new(&extension.to_ascii_uppercase())),
                Some(kind)
            );
            assert_eq!(
                DocumentKind::parse_extension(OsStr::new(&format!(".{extension}"))),
                Some(kind)
            );
        }
    }

    #[test]
    fn unsupported_and_malformed_extensions_are_rejected() {
        for extension in ["", ".", "doc", "odt", "docx.pdf", " docx", "docx "] {
            assert_eq!(DocumentKind::parse_extension(OsStr::new(extension)), None);
        }
    }

    #[test]
    fn new_request_has_exact_argument_shape_and_round_trips() {
        for kind in [
            DocumentKind::Docx,
            DocumentKind::Xlsx,
            DocumentKind::Pptx,
            DocumentKind::Pdf,
        ] {
            let request = LaunchRequest::New(kind);
            assert_eq!(request.editor_args(), strings(&["--new", kind.extension()]));
            assert_eq!(
                parse_editor_args(request.editor_args()).unwrap(),
                Some(request)
            );
        }
    }

    #[test]
    fn open_request_preserves_spaces_and_round_trips() {
        let request = LaunchRequest::Open(PathBuf::from(
            r"C:\Users\Example User\Documents\Quarterly report.xlsx",
        ));
        assert_eq!(
            request.editor_args(),
            strings(&[
                "--open",
                r"C:\Users\Example User\Documents\Quarterly report.xlsx"
            ])
        );
        assert_eq!(
            parse_editor_args(request.editor_args()).unwrap(),
            Some(request)
        );
    }

    #[test]
    fn open_dialog_has_exact_argument_shape_and_round_trips() {
        let request = LaunchRequest::OpenDialog;
        assert_eq!(request.editor_args(), strings(&["--open-dialog"]));
        assert_eq!(
            parse_editor_args(request.editor_args()).unwrap(),
            Some(request)
        );
    }

    #[test]
    fn unrelated_makepad_arguments_are_ignored() {
        let parsed = parse_editor_args(strings(&[
            "--d3d",
            "--new",
            "xlsx",
            "--windowed",
            "--log-level",
            "debug",
        ]))
        .unwrap();
        assert_eq!(parsed, Some(LaunchRequest::New(DocumentKind::Xlsx)));

        assert_eq!(
            parse_editor_args(strings(&["--d3d", "--windowed"])).unwrap(),
            None
        );
    }

    #[test]
    fn missing_and_empty_values_are_rejected() {
        assert!(parse_editor_args(strings(&["--new"])).is_err());
        assert!(parse_editor_args(strings(&["--open"])).is_err());
        assert!(parse_editor_args(strings(&["--new", ""])).is_err());
        assert!(parse_editor_args(strings(&["--open", ""])).is_err());
    }

    #[test]
    fn invalid_new_formats_are_rejected() {
        for extension in ["odt", "doc", "docx.pdf", "--open-dialog"] {
            assert!(parse_editor_args(strings(&["--new", extension])).is_err());
        }
    }

    #[test]
    fn duplicate_and_conflicting_requests_are_rejected() {
        let cases = [
            strings(&["--open-dialog", "--open-dialog"]),
            strings(&["--new", "docx", "--new", "docx"]),
            strings(&["--new", "docx", "--open", "file.docx"]),
            strings(&["--open", "file.docx", "--open-dialog"]),
            strings(&["--open-dialog", "--open", "file.docx"]),
        ];

        for args in cases {
            assert!(parse_editor_args(args).is_err());
        }
    }

    #[test]
    fn open_paths_that_look_like_flags_are_still_values() {
        let request = LaunchRequest::Open(PathBuf::from("--not-a-makepad-flag"));
        assert_eq!(
            parse_editor_args(request.editor_args()).unwrap(),
            Some(request)
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_unix_path_bytes_are_preserved() {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_vec(vec![
            b'/', b't', b'm', b'p', b'/', b'r', b'e', b'p', b'o', b'r', b't', b' ', 0xff,
        ]));
        let request = LaunchRequest::Open(path.clone());
        let args = request.editor_args();

        assert_eq!(args[0], OsStr::new("--open"));
        assert_eq!(args[1], path.as_os_str());
        assert_eq!(parse_editor_args(args).unwrap(), Some(request));
    }

    #[cfg(windows)]
    #[test]
    fn non_unicode_windows_path_units_are_preserved() {
        use std::os::windows::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_wide(&[
            b'C' as u16,
            b':' as u16,
            b'\\' as u16,
            b'R' as u16,
            b'e' as u16,
            b'p' as u16,
            b'o' as u16,
            b'r' as u16,
            b't' as u16,
            b' ' as u16,
            0xd800,
        ]));
        let request = LaunchRequest::Open(path.clone());
        let args = request.editor_args();

        assert_eq!(args[0], OsStr::new("--open"));
        assert_eq!(args[1], path.as_os_str());
        assert_eq!(parse_editor_args(args).unwrap(), Some(request));
    }

    #[test]
    fn editor_path_is_a_sibling_with_the_platform_executable_suffix() {
        let directory = Path::new("install").join("Office");
        let path = sibling_editor_path(&directory.join("launcher.exe"));
        assert_eq!(path.parent(), Some(directory.as_path()));

        #[cfg(windows)]
        assert_eq!(path.file_name(), Some(OsStr::new("rudra-office-app.exe")));
        #[cfg(not(windows))]
        assert_eq!(path.file_name(), Some(OsStr::new("rudra-office-app")));
    }
}
