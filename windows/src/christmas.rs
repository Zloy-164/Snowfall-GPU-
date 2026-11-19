//! Click-through Christmas tree decoration, independent of the snowfall renderer.
//! Uses a small colour-keyed Win32 layered window and a 350 ms light animation.
//! Everything stays local; no textures or network resources are required.
use std::cell::{Cell, RefCell};
use std::mem::size_of;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, Ellipse, EndPaint, FillRect, HGDIOBJ, PAINTSTRUCT, Polygon, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("SnowfallGPUChristmasTree");
const CLEAR: COLORREF = COLORREF(0x0003_0201);
const TIMER_LIGHTS: usize = 1;

#[derive(Clone, Copy)]
struct Config { on: bool, size: u32, lights: bool, frame: u32 }
thread_local! {
    static WINDOW: Cell<isize> = const { Cell::new(0) };
    static CONFIG: RefCell<Config> = const { RefCell::new(Config { on: false, size: 100, lights: true, frame: 0 }) };
}

/// Enable or update the tree from the main UI thread. Size is 60–160%.
pub fn configure(on: bool, size: u32, lights: bool) {
    CONFIG.with_borrow_mut(|v| { v.on = on; v.size = size.clamp(60, 160); v.lights = lights; });
    WINDOW.with(|slot| unsafe {
        let mut hwnd = HWND(slot.get() as _);
        if hwnd.0.is_null() {
            if !on { return; }
            let Ok(instance) = GetModuleHandleW(None) else { return };
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(procedure),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class);
            let Ok(created) = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                CLASS, w!("Snowfall GPU tree"), WS_POPUP,
                0, 0, 0, 0, None, None, Some(instance.into()), None
            ) else { return };
            hwnd = created;
            slot.set(hwnd.0 as isize);
            let _ = SetLayeredWindowAttributes(hwnd, CLEAR, 0, LWA_COLORKEY);
            SetTimer(Some(hwnd), TIMER_LIGHTS, 350, None);
        }
        let scale = CONFIG.with_borrow(|v| v.size) as i32;
        let width = 190 * scale / 100;
        let height = 230 * scale / 100;
        let x = GetSystemMetrics(SM_CXSCREEN) - width - 22;
        let y = GetSystemMetrics(SM_CYSCREEN) - height - 62;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        if on {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = InvalidateRect(Some(hwnd), None, true);
        } else {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    });
}

pub fn shutdown() {
    WINDOW.with(|slot| {
        let raw = slot.replace(0);
        if raw != 0 { unsafe { let _ = DestroyWindow(HWND(raw as _)); } }
    });
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, _w: WPARAM, _l: LPARAM) -> LRESULT {
    match message {
        WM_TIMER => {
            CONFIG.with_borrow_mut(|v| v.frame = v.frame.wrapping_add(1));
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => unsafe { DefWindowProcW(hwnd, message, _w, _l) },
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

fn paint(hwnd: HWND) {
    let (size, lights, frame) = CONFIG.with_borrow(|v| (v.size, v.lights, v.frame));
    let scale = size as f32 / 100.0;
    let p = |x: f32| (x * scale) as i32;
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let background = CreateSolidBrush(CLEAR);
        let rect = RECT { left: 0, top: 0, right: p(190.0), bottom: p(230.0) };
        FillRect(hdc, &rect, background);
        let _ = DeleteObject(HGDIOBJ(background.0));

        let trunk = CreateSolidBrush(rgb(106, 67, 43));
        FillRect(hdc, &RECT { left: p(82.0), top: p(169.0), right: p(108.0), bottom: p(208.0) }, trunk);
        let _ = DeleteObject(HGDIOBJ(trunk.0));

        for (top, bottom, width, green) in [(26.0, 112.0, 62.0, rgb(18, 95, 52)),
                                             (63.0, 153.0, 83.0, rgb(17, 117, 62)),
                                             (102.0, 195.0, 98.0, rgb(13, 87, 51))] {
            let brush = CreateSolidBrush(green);
            let old = SelectObject(hdc, HGDIOBJ(brush.0));
            let points = [
                POINT { x: p(95.0), y: p(top) },
                POINT { x: p(95.0 - width), y: p(bottom) },
                POINT { x: p(95.0 + width), y: p(bottom) }
            ];
            let _ = Polygon(hdc, &points);
            let _ = SelectObject(hdc, old);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }

        // Five-pointed golden topper.
        let star = CreateSolidBrush(rgb(255, 217, 71));
        let old = SelectObject(hdc, HGDIOBJ(star.0));
        let points: Vec<POINT> = (0..10).map(|i| {
            let a = (i as f32 * 36.0 - 90.0).to_radians();
            let r = if i % 2 == 0 { 17.0 } else { 7.2 };
            POINT { x: p(95.0 + a.cos() * r), y: p(23.0 + a.sin() * r) }
        }).collect();
        let _ = Polygon(hdc, &points);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(star.0));

        let ornaments: [(f32, f32); 13] = [
            (93.0, 65.0), (74.0, 95.0), (117.0, 104.0),
            (60.0, 132.0), (91.0, 125.0), (128.0, 137.0),
            (45.0, 168.0), (74.0, 158.0), (105.0, 165.0),
            (142.0, 170.0), (77.0, 186.0), (123.0, 186.0), (97.0, 144.0)
        ];
        for (i, &(x, y)) in ornaments.iter().enumerate() {
            let bright = !lights || (frame + i as u32) % 4 != 0;
            let color = match i % 4 {
                0 => if bright { rgb(255, 74, 73) } else { rgb(122, 34, 31) },
                1 => if bright { rgb(255, 222, 96) } else { rgb(121, 104, 42) },
                2 => if bright { rgb(92, 194, 255) } else { rgb(35, 86, 122) },
                _ => if bright { rgb(249, 146, 245) } else { rgb(107, 52, 102) }
            };
            let brush = CreateSolidBrush(color);
            let old = SelectObject(hdc, HGDIOBJ(brush.0));
            let radius = if bright { 5.0 } else { 3.5 };
            let _ = Ellipse(hdc, p(x - radius), p(y - radius), p(x + radius), p(y + radius));
            let _ = SelectObject(hdc, old);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
        EndPaint(hwnd, &ps);
    }
}
