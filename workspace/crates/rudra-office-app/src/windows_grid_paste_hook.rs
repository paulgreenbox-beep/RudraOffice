//! Consumer-owned Ctrl+V routing BEFORE Makepad's unbounded native text read.
//! Only a focused Sheets grid opts in. TextInputs retain native clipboard input.
//! One HWND hook, one bounded request slot; no widget/viewport/history owner.
use crate::{CellRange, RuntimeDocumentId, SignalToUI};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use windows_uia::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{GetWindowThreadProcessId, WM_KEYDOWN, WM_NCDESTROY},
    },
};

const HOOK_ID: usize = 0x52435028;
const KEY_V: usize = 0x56;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PasteTarget {
    pub document: RuntimeDocumentId,
    pub range: CellRange,
}

#[derive(Default)]
struct HookState {
    target: Option<PasteTarget>,
    pending: Option<PasteTarget>,
    intercepted: u64,
}

thread_local! {
    // SetWindowSubclass must run on the HWND's UI thread. No raw Rust pointers
    // cross FFI and no arbitrary UIA worker thread can mutate this registry.
    static STATES: RefCell<BTreeMap<isize, Rc<RefCell<HookState>>>> = const { RefCell::new(BTreeMap::new()) };
}

fn routes_to_grid(target: bool, key: usize, control: bool, alt: bool) -> bool {
    target && key == KEY_V && control && !alt
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        STATES.with(|states| {
            if let Ok(mut states) = states.try_borrow_mut() {
                states.remove(&(hwnd.0 as isize));
            }
        });
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass), HOOK_ID);
        }
    } else if message == WM_KEYDOWN {
        let state = STATES.with(|states| {
            states
                .try_borrow()
                .ok()
                .and_then(|states| states.get(&(hwnd.0 as isize)).cloned())
        });
        if let Some(state) = state {
            let control = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
            let alt = unsafe { GetKeyState(VK_MENU.0 as i32) } < 0;
            let routed = if let Ok(mut state) = state.try_borrow_mut() {
                if routes_to_grid(state.target.is_some(), wparam.0, control, alt) {
                    state.intercepted = state.intercepted.saturating_add(1);
                    // A held shortcut cannot enqueue an unbounded batch.
                    if lparam.0 & (1 << 30) == 0 {
                        state.pending = state.target.clone();
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if routed {
                SignalToUI::set_ui_signal();
                // Do NOT delegate: Makepad otherwise allocates GlobalSize bytes
                // and decodes them before delivering its pasted TextInput.
                return LRESULT(0);
            }
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

pub(super) struct NativeGridPasteHook {
    hwnd: HWND,
    state: Rc<RefCell<HookState>>,
}

impl NativeGridPasteHook {
    pub fn install(hwnd: HWND) -> Result<Self, String> {
        if unsafe { GetWindowThreadProcessId(hwnd, None) } != unsafe { GetCurrentThreadId() } {
            return Err("bounded grid Paste hook must be installed on the window UI thread".into());
        }
        let state = Rc::new(RefCell::new(HookState::default()));
        let inserted = STATES.with(|states| {
            let mut states = states.borrow_mut();
            if states.contains_key(&(hwnd.0 as isize)) {
                return false;
            }
            states.insert(hwnd.0 as isize, state.clone());
            true
        });
        if !inserted {
            return Err("duplicate grid Paste owner for this HWND".into());
        }
        if !unsafe { SetWindowSubclass(hwnd, Some(subclass), HOOK_ID, 0) }.as_bool() {
            STATES.with(|states| {
                states.borrow_mut().remove(&(hwnd.0 as isize));
            });
            return Err("could not install bounded pre-allocation grid Paste owner".into());
        }
        Ok(Self { hwnd, state })
    }

    pub fn update_target(&self, target: Option<PasteTarget>) {
        self.state.borrow_mut().target = target;
    }
    pub fn take_pending(&self) -> Option<PasteTarget> {
        self.state.borrow_mut().pending.take()
    }
    pub fn intercepted(&self) -> u64 {
        self.state.borrow().intercepted
    }
}

impl Drop for NativeGridPasteHook {
    fn drop(&mut self) {
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(subclass), HOOK_ID);
        }
        STATES.with(|states| {
            states.borrow_mut().remove(&(self.hwnd.0 as isize));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preallocation_owner_only_consumes_ctrl_v_in_opted_in_grid() {
        assert!(routes_to_grid(true, KEY_V, true, false));
        assert!(!routes_to_grid(false, KEY_V, true, false));
        assert!(!routes_to_grid(true, KEY_V, false, false));
        assert!(!routes_to_grid(true, KEY_V, true, true));
        assert!(!routes_to_grid(true, 0x43, true, false));
    }
}
