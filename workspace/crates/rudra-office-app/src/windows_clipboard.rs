//! Bounded Windows `CF_UNICODETEXT` access for the grid clipboard adapter.
//!
//! Windows synthesizes additional text formats at CloseClipboard and changes
//! its sequence there. After our write closes, a second bounded read lock must
//! prove BOTH original owner HWND and original CF_UNICODETEXT HGLOBAL identity
//! before claiming the final sequence. A racing replacement, even identical
//! text, must not be attributed to this write. The UI thread serializes our own
//! writes; the adapter is not a security boundary against HWND/handle spoofing.
//!
//! This writer prepares its memory before opening or emptying the clipboard.
//! Once `EmptyClipboard` succeeds, a later `SetClipboardData` failure cannot
//! restore the previous clipboard contents; in that case the clipboard may be
//! left empty. No private clipboard format is used.

use std::char;

const HARD_MAX_CLIPBOARD_TEXT_BYTES: usize = 16 * 1024 * 1024;
// GlobalSize may report allocator rounding beyond the requested UTF-16 block.
// This slack is bounded and is not counted as text; decoded UTF-8 is checked
// against max_bytes before allocating the returned String.
const GLOBAL_SIZE_ROUNDING_SLACK: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ClipboardObservation {
    pub text: String,
    pub sequence: u32,
}

pub(super) fn read_text(max_bytes: usize) -> Result<Option<ClipboardObservation>, String> {
    validate_max_bytes(max_bytes)?;
    platform::read_text(max_bytes)
}

pub(super) fn write_text_verified(
    text: &str,
    max_bytes: usize,
) -> Result<ClipboardObservation, String> {
    validate_max_bytes(max_bytes)?;
    validate_write_text(text, max_bytes)?;
    platform::write_text_verified(text, max_bytes)
}

pub(super) fn has_text() -> bool {
    platform::has_text()
}

pub(super) fn current_sequence() -> Option<u32> {
    platform::current_sequence()
}

fn validate_max_bytes(max_bytes: usize) -> Result<(), String> {
    if max_bytes > HARD_MAX_CLIPBOARD_TEXT_BYTES {
        return Err("clipboard text limit exceeds the adapter hard limit".to_owned());
    }
    Ok(())
}

fn validate_write_text(text: &str, max_bytes: usize) -> Result<(), String> {
    if text.len() > max_bytes {
        return Err("clipboard text exceeds the requested byte limit".to_owned());
    }
    if text.contains('\0') {
        return Err("clipboard text contains an embedded NUL character".to_owned());
    }
    Ok(())
}

fn max_global_size(max_bytes: usize) -> Result<usize, String> {
    validate_max_bytes(max_bytes)?;
    max_bytes
        .checked_add(1)
        .and_then(|units| units.checked_mul(2))
        .and_then(|bytes| bytes.checked_add(GLOBAL_SIZE_ROUNDING_SLACK))
        .ok_or_else(|| "clipboard storage limit overflowed".to_owned())
}

fn decode_clipboard_units(units: &[u16], max_bytes: usize) -> Result<String, String> {
    validate_max_bytes(max_bytes)?;
    let payload_len = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());

    // Validate all UTF-16 and calculate the output size before allocating.
    let mut utf8_len = 0usize;
    for decoded in char::decode_utf16(units[..payload_len].iter().copied()) {
        let character = decoded.map_err(|_| "clipboard contains malformed UTF-16".to_owned())?;
        utf8_len = utf8_len
            .checked_add(character.len_utf8())
            .ok_or_else(|| "clipboard text length overflowed".to_owned())?;
        if utf8_len > max_bytes {
            return Err("clipboard text exceeds the requested byte limit".to_owned());
        }
    }

    let mut text = String::new();
    text.try_reserve_exact(utf8_len)
        .map_err(|_| "could not allocate bounded clipboard text".to_owned())?;
    // The first pass validated this same immutable slice and established capacity.
    for character in char::decode_utf16(units[..payload_len].iter().copied()).flatten() {
        text.push(character);
    }
    Ok(text)
}

fn decode_terminated_clipboard_block(units: &[u16], max_bytes: usize) -> Result<String, String> {
    if !units.contains(&0) {
        return Err("clipboard CF_UNICODETEXT data is not NUL-terminated".to_owned());
    }
    decode_clipboard_units(units, max_bytes)
}

fn verify_observation(
    previous_sequence: u32,
    expected_text: &str,
    observation: Option<ClipboardObservation>,
) -> Result<ClipboardObservation, String> {
    let observation =
        observation.ok_or_else(|| "clipboard text could not be read back".to_owned())?;
    if observation.sequence == 0 {
        return Err("clipboard sequence is unavailable; write is unverified".to_owned());
    }
    if observation.sequence == previous_sequence {
        return Err("clipboard sequence did not change; write is unverified".to_owned());
    }
    if observation.text != expected_text {
        return Err("clipboard read-back does not match the requested text".to_owned());
    }
    Ok(observation)
}

fn verify_owned_observation(
    previous_sequence: u32,
    expected_text: &str,
    observation: Option<ClipboardObservation>,
    original_owner_and_buffer: bool,
) -> Result<ClipboardObservation, String> {
    if !original_owner_and_buffer {
        return Err("clipboard owner or text buffer changed before ownership confirmation".into());
    }
    verify_observation(previous_sequence, expected_text, observation)
}

#[cfg(all(windows, not(test)))]
mod platform {
    use super::{
        ClipboardObservation, decode_terminated_clipboard_block, max_global_size,
        validate_write_text, verify_observation, verify_owned_observation,
    };
    use std::{mem, ptr};
    use windows_uia::Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner,
                GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
                SetClipboardData,
            },
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Ole::CF_UNICODETEXT,
            Threading::GetCurrentProcess,
        },
        UI::Input::KeyboardAndMouse::GetActiveWindow,
    };

    struct ClipboardLock(bool);

    impl ClipboardLock {
        fn open(owner: Option<HWND>) -> Result<Self, String> {
            unsafe { OpenClipboard(owner) }
                .map_err(|error| format!("OpenClipboard failed: {error}"))?;
            Ok(Self(true))
        }

        fn close(mut self) -> Result<(), String> {
            unsafe { CloseClipboard() }
                .map_err(|error| format!("CloseClipboard failed: {error}"))?;
            self.0 = false;
            Ok(())
        }
    }

    impl Drop for ClipboardLock {
        fn drop(&mut self) {
            if self.0 {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }
    }

    struct GlobalLockGuard(HGLOBAL);

    impl Drop for GlobalLockGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = GlobalUnlock(self.0);
            }
        }
    }

    struct GlobalAllocation {
        handle: HGLOBAL,
        transferred: bool,
    }

    impl GlobalAllocation {
        fn allocate(bytes: usize) -> Result<Self, String> {
            let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }
                .map_err(|error| format!("GlobalAlloc failed for clipboard text: {error}"))?;
            if handle.0.is_null() {
                return Err("GlobalAlloc failed for clipboard text".to_owned());
            }
            Ok(Self {
                handle,
                transferred: false,
            })
        }

        fn copy_utf16(&self, units: &[u16]) -> Result<(), String> {
            let destination = unsafe { GlobalLock(self.handle) };
            if destination.is_null() {
                return Err("GlobalLock failed for clipboard text".to_owned());
            }
            let _lock = GlobalLockGuard(self.handle);
            // SAFETY: this HGLOBAL was allocated for exactly enough bytes for
            // `units`; the source slice is valid and does not overlap it.
            unsafe {
                ptr::copy_nonoverlapping(units.as_ptr(), destination.cast::<u16>(), units.len());
            }
            Ok(())
        }
    }

    impl Drop for GlobalAllocation {
        fn drop(&mut self) {
            if !self.transferred {
                unsafe {
                    let _ = GlobalFree(Some(self.handle));
                }
            }
        }
    }

    pub(super) fn read_text(max_bytes: usize) -> Result<Option<ClipboardObservation>, String> {
        let _clipboard = ClipboardLock::open(None)?;
        let before = unsafe { GetClipboardSequenceNumber() };
        let text = read_open_clipboard(max_bytes)?;
        let after = unsafe { GetClipboardSequenceNumber() };
        if before != after {
            return Err("clipboard changed while its text was being read".to_owned());
        }
        Ok(text.map(|text| ClipboardObservation {
            text,
            sequence: after,
        }))
    }

    pub(super) fn write_text_verified(
        text: &str,
        max_bytes: usize,
    ) -> Result<ClipboardObservation, String> {
        validate_write_text(text, max_bytes)?;

        // Build and fill the movable block before opening or emptying the
        // clipboard, so validation/allocation failures preserve prior data.
        let mut units = Vec::new();
        units
            .try_reserve_exact(text.len().saturating_add(1))
            .map_err(|_| "could not allocate bounded UTF-16 clipboard text".to_owned())?;
        units.extend(text.encode_utf16());
        units.push(0);
        let bytes = units
            .len()
            .checked_mul(mem::size_of::<u16>())
            .ok_or_else(|| "clipboard storage size overflowed".to_owned())?;
        let allocation = GlobalAllocation::allocate(bytes)?;
        allocation.copy_utf16(&units)?;

        // `OpenClipboard(NULL)` followed by EmptyClipboard can leave no owner
        // and make SetClipboardData fail. The grid calls this on its UI thread,
        // where the active window is the correct clipboard owner.
        let owner = unsafe { GetActiveWindow() };
        if owner.0.is_null() {
            return Err("no active window is available to own the clipboard".to_owned());
        }
        let clipboard = ClipboardLock::open(Some(owner))?;
        let before = unsafe { GetClipboardSequenceNumber() };

        unsafe { EmptyClipboard() }.map_err(|error| format!("EmptyClipboard failed: {error}"))?;
        let transferred =
            unsafe { SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(allocation.handle.0))) }
                .map_err(|error| {
                    format!("SetClipboardData failed after EmptyClipboard: {error}")
                })?;
        if transferred.0.is_null() {
            return Err("SetClipboardData returned no clipboard handle".to_owned());
        }

        // Windows owns the HGLOBAL after SetClipboardData succeeds.
        let mut allocation = allocation;
        allocation.transferred = true;

        // Verify our direct write before allowing Windows to synthesize its
        // standard CF_TEXT/CF_OEMTEXT/CF_LOCALE formats at CloseClipboard.
        let observation = read_open_clipboard(max_bytes)?.map(|text| ClipboardObservation {
            text,
            sequence: unsafe { GetClipboardSequenceNumber() },
        });
        let observation = verify_observation(before, text, observation)?;
        let final_sequence = unsafe { GetClipboardSequenceNumber() };
        if final_sequence != observation.sequence {
            return Err("clipboard sequence changed during write verification".to_owned());
        }
        clipboard.close()?;
        let _read_lock = ClipboardLock::open(None)?;
        let observed_owner = unsafe { GetClipboardOwner() }.ok();
        let observed_buffer = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }.ok();
        let own_identity =
            observed_owner == Some(owner) && observed_buffer == Some(HANDLE(allocation.handle.0));
        let read_sequence = unsafe { GetClipboardSequenceNumber() };
        let final_observation = read_open_clipboard(max_bytes)?.map(|text| ClipboardObservation {
            text,
            sequence: unsafe { GetClipboardSequenceNumber() },
        });
        if final_observation
            .as_ref()
            .is_some_and(|observation| observation.sequence != read_sequence)
        {
            return Err("clipboard sequence changed during post-close readback".into());
        }
        verify_owned_observation(before, text, final_observation, own_identity)
    }

    pub(super) fn has_text() -> bool {
        unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok() }
    }

    pub(super) fn current_sequence() -> Option<u32> {
        let sequence = unsafe { GetClipboardSequenceNumber() };
        (sequence != 0).then_some(sequence)
    }

    fn read_open_clipboard(max_bytes: usize) -> Result<Option<String>, String> {
        if !unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok() } {
            return Ok(None);
        }

        let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }
            .map_err(|error| format!("GetClipboardData failed: {error}"))?;
        if handle.0.is_null() {
            return Err("GetClipboardData returned a null handle".to_owned());
        }
        let global = HGLOBAL(handle.0);
        let size = unsafe { GlobalSize(global) };
        let bound = max_global_size(max_bytes)?;
        if size < mem::size_of::<u16>() || size % mem::size_of::<u16>() != 0 {
            return Err("clipboard UTF-16 storage has an invalid size".to_owned());
        }
        if size > bound {
            return Err("clipboard UTF-16 storage exceeds the bounded read limit".to_owned());
        }

        let source = unsafe { GlobalLock(global) };
        if source.is_null() {
            return Err("GlobalLock failed for clipboard text".to_owned());
        }
        let _lock = GlobalLockGuard(global);
        // Copy through the OS into initialized storage instead of creating a
        // Rust slice over the whole GlobalSize allocation. GlobalSize can
        // include uninitialized allocator padding after a malformed unterminated
        // string, which must never be read as a Rust `u16`.
        let storage_units = size / mem::size_of::<u16>();
        let mut units = Vec::new();
        units
            .try_reserve_exact(storage_units)
            .map_err(|_| "could not allocate bounded clipboard storage".to_owned())?;
        units.resize(storage_units, 0u16);
        let mut bytes_read = 0usize;
        unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                source.cast_const(),
                units.as_mut_ptr().cast(),
                size,
                Some(&mut bytes_read),
            )
        }
        .map_err(|error| format!("could not copy bounded clipboard storage: {error}"))?;
        if bytes_read != size {
            return Err("clipboard storage could not be copied completely".to_owned());
        }
        decode_terminated_clipboard_block(&units, max_bytes).map(Some)
    }
}

#[cfg(any(not(windows), test))]
mod platform {
    use super::ClipboardObservation;

    pub(super) fn read_text(_max_bytes: usize) -> Result<Option<ClipboardObservation>, String> {
        Err("Windows clipboard access is unavailable in this build".to_owned())
    }

    pub(super) fn write_text_verified(
        _text: &str,
        _max_bytes: usize,
    ) -> Result<ClipboardObservation, String> {
        Err("Windows clipboard access is unavailable in this build".to_owned())
    }

    pub(super) fn has_text() -> bool {
        false
    }

    pub(super) fn current_sequence() -> Option<u32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClipboardObservation, decode_clipboard_units, decode_terminated_clipboard_block,
        max_global_size, validate_max_bytes, validate_write_text, verify_observation,
        verify_owned_observation,
    };

    #[test]
    fn decodes_cyrillic_and_emoji_and_stops_at_nul() {
        let units = [
            0x0420, 0x0443, 0x0441, 0x0441, 0x043A, 0x0438, 0x0439, 0x20, 0xD83D, 0xDE00, 0, 0xD800,
        ];
        assert_eq!(decode_clipboard_units(&units, 64).unwrap(), "Русский 😀");
    }

    #[test]
    fn accepts_unterminated_well_formed_utf16() {
        assert_eq!(decode_clipboard_units(&[0x0416, 0x0430], 4).unwrap(), "Жа");
    }

    #[test]
    fn clipboard_block_requires_a_terminator() {
        assert_eq!(decode_terminated_clipboard_block(&[0], 0).unwrap(), "");
        assert!(decode_terminated_clipboard_block(&[0x0416, 0x0430], 4).is_err());
    }

    #[test]
    fn rejects_malformed_utf16_before_terminator() {
        assert!(decode_clipboard_units(&[0xD800, 0], 8).is_err());
        assert!(decode_clipboard_units(&[0xDC00], 8).is_err());
    }

    #[test]
    fn checks_utf8_output_limit_before_returning_text() {
        assert!(decode_clipboard_units(&[0xD83D, 0xDE00, 0], 3).is_err());
        assert_eq!(
            decode_clipboard_units(&[0xD83D, 0xDE00, 0], 4).unwrap(),
            "😀"
        );
    }

    #[test]
    fn bounds_global_storage_and_rejects_excessive_limit() {
        assert_eq!(max_global_size(0).unwrap(), 2 + 4096);
        assert!(validate_max_bytes(16 * 1024 * 1024 + 1).is_err());
        assert!(max_global_size(usize::MAX).is_err());
    }

    #[test]
    fn write_validation_rejects_oversize_and_embedded_nul() {
        assert!(validate_write_text("abcd", 3).is_err());
        assert!(validate_write_text("a\0b", 8).is_err());
        assert!(validate_write_text("", 0).is_ok());
    }

    #[test]
    fn verification_requires_changed_sequence_and_exact_text() {
        let verified = verify_observation(
            41,
            "Русский 😀",
            Some(ClipboardObservation {
                text: "Русский 😀".to_owned(),
                sequence: 42,
            }),
        )
        .unwrap();
        assert_eq!(verified.sequence, 42);
        assert!(
            verify_observation(
                0,
                "Русский 😀",
                Some(ClipboardObservation {
                    text: "Русский 😀".to_owned(),
                    sequence: 1,
                }),
            )
            .is_ok()
        );

        assert!(
            verify_observation(
                41,
                "Русский 😀",
                Some(ClipboardObservation {
                    text: "Русский 😀".to_owned(),
                    sequence: 41,
                }),
            )
            .is_err()
        );
        assert!(
            verify_observation(
                41,
                "Русский 😀",
                Some(ClipboardObservation {
                    text: "other".to_owned(),
                    sequence: 42,
                }),
            )
            .is_err()
        );
        assert!(verify_observation(41, "Русский 😀", None).is_err());
        assert!(
            verify_observation(
                0,
                "Русский 😀",
                Some(ClipboardObservation {
                    text: "Русский 😀".to_owned(),
                    sequence: 0,
                }),
            )
            .is_err()
        );
    }

    #[test]
    fn windows_synthesized_sequence_is_claimed_only_with_original_owner_and_buffer() {
        let text = "Русский 😀";
        let post_close = Some(ClipboardObservation {
            text: text.into(),
            sequence: 384,
        });
        assert_eq!(
            verify_owned_observation(379, text, post_close.clone(), true)
                .unwrap()
                .sequence,
            384
        );
        // Identical replacement text is insufficient without original native identity.
        assert!(verify_owned_observation(379, text, post_close, false).is_err());
    }
}
