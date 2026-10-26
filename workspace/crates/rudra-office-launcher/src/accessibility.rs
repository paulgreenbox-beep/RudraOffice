//! Shared consumer wiring; the generic UIA provider remains in Toolkit.
//!
//! The generic provider lives in `toolkit-windows-automation`. This module only
//! locates this application's Makepad window, converts Makepad layout units to
//! UIA screen pixels, and marshals semantic actions back onto the UI thread.

use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

use makepad_widgets::{Cx, Rect};
use toolkit_windows_automation::{
    AccessibilityActionHandler, AccessibilityActionRequest, AccessibilityRect, AccessibilityTree,
    WindowsAccessibilityBridge,
};
use windows_uia::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, TRUE},
        Graphics::Gdi::ClientToScreen,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
                GetWindowThreadProcessId,
            },
        },
    },
    core::BOOL,
};

thread_local! {
    static BRIDGE: RefCell<Option<WindowsAccessibilityBridge>> = const { RefCell::new(None) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// A bridge action posted back into Makepad's normal `Actions` queue.
#[derive(Clone, Debug)]
pub struct AccessibilityUiAction(pub AccessibilityActionRequest);

/// A resolved Makepad window and its logical-to-screen coordinate transform.
pub struct AccessibilityWindow {
    hwnd: HWND,
    origin: POINT,
    dpi_factor: f64,
    bounds: AccessibilityRect,
}

impl AccessibilityWindow {
    pub fn bounds(&self) -> AccessibilityRect {
        self.bounds
    }

    pub fn screen_rect(&self, rect: Rect) -> AccessibilityRect {
        AccessibilityRect {
            x: self.origin.x as f64 + rect.pos.x * self.dpi_factor,
            y: self.origin.y as f64 + rect.pos.y * self.dpi_factor,
            width: (rect.size.x * self.dpi_factor).max(0.0),
            height: (rect.size.y * self.dpi_factor).max(0.0),
        }
    }
}

/// Resolves only the Rudra Office window that belongs to this process.
pub fn current_window(window_title: &str) -> Result<AccessibilityWindow, String> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let handles = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        handles.push(hwnd);
        TRUE
    }
    let mut handles = Vec::new();
    unsafe {
        EnumWindows(
            Some(collect),
            LPARAM((&mut handles as *mut Vec<HWND>) as isize),
        )
    }
    .map_err(|error| error.to_string())?;
    let hwnd = handles
        .into_iter()
        .find(|hwnd| {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(*hwnd, Some(&mut pid)) };
            if pid != std::process::id() {
                return false;
            }
            let len = unsafe { GetWindowTextLengthW(*hwnd) }.max(0) as usize;
            let mut text = vec![0u16; len.saturating_add(1)];
            let n = unsafe { GetWindowTextW(*hwnd, &mut text) }.max(0) as usize;
            String::from_utf16_lossy(&text[..n]) == window_title
        })
        .ok_or_else(|| format!("No {window_title} window in the current process"))?;

    let mut origin = POINT::default();
    unsafe { ClientToScreen(hwnd, &mut origin) }
        .ok()
        .map_err(|error| error.to_string())?;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|error| error.to_string())?;
    let dpi_factor = unsafe { GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0;

    Ok(AccessibilityWindow {
        hwnd,
        origin,
        dpi_factor,
        bounds: AccessibilityRect {
            x: rect.left as f64,
            y: rect.top as f64,
            width: (rect.right - rect.left).max(0) as f64,
            height: (rect.bottom - rect.top).max(0) as f64,
        },
    })
}

/// Publishes the latest semantic snapshot and keeps the generic provider alive.
pub fn publish(tree: AccessibilityTree, window: &AccessibilityWindow) {
    let tree = GENERATION.with(|generation| {
        let next = generation.get().wrapping_add(1);
        generation.set(next);
        tree.with_generation(next)
    });

    BRIDGE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(bridge) = slot.as_ref() {
            if let Err(error) = bridge.update(tree) {
                eprintln!("Rudra accessibility bridge update failed: {error}");
            }
            return;
        }

        let handler: AccessibilityActionHandler = Arc::new(|request| {
            Cx::post_action(AccessibilityUiAction(request));
            Ok(())
        });
        match WindowsAccessibilityBridge::attach(window.hwnd, tree, handler) {
            Ok(bridge) => *slot = Some(bridge),
            Err(error) => eprintln!("Rudra accessibility bridge attach failed: {error}"),
        }
    });
}
