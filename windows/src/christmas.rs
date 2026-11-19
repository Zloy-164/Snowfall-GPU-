//! A richer click-through evergreen decoration, with tiny independent snow physics.
//! The falling snow in this overlay is approximated locally; the desktop simulation
//! remains unchanged. A 66 ms timer caps redraws when the tree is visible.
use std::cell::{Cell, RefCell};
use std::f32::consts::PI;
use std::mem::size_of;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, Ellipse, EndPaint, FillRect, HDC, HGDIOBJ,
    InvalidateRect, PAINTSTRUCT, Polygon, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("SnowfallGPUChristmasTree");
const CLEAR: COLORREF = COLORREF(0x0003_0201);
const TIMER: usize = 1;
const LEVELS: [f32; 5] = [102.0, 146.0, 188.0, 234.0, 277.0];
const REACH: [f32; 5] = [40.0, 58.0, 78.0, 98.0, 116.0];

#[derive(Clone, Copy)]
struct Config {
    on: bool,
    size: u32,
    lights: bool,
    garland: u32,
    snow: bool,
    snowfall: bool,
    intensity: u32,
}

#[derive(Clone, Copy)]
struct Flake {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    sliding: bool,
}

struct Scene {
    cfg: Config,
    frame: u32,
    seed: u32,
    piles: [f32; 5],
    flakes: Vec<Flake>,
}

thread_local! {
    static WINDOW: Cell<isize> = const { Cell::new(0) };
    static SCENE: RefCell<Scene> = const { RefCell::new(Scene {
        cfg: Config { on: false, size: 100, lights: true, garland: 0, snow: true, snowfall: true, intensity: 40 },
        frame: 0,
        seed: 0x5EED_1234,
        piles: [0.10; 5],
        flakes: Vec::new(),
    }) };
}

/// Settings persist in prefs.rs; this function only updates the visible effect.
pub fn configure(on: bool, size: u32, lights: bool, garland: u32, snow: bool, snowfall: bool, intensity: u32) {
    SCENE.with_borrow_mut(|scene| {
        if !scene.cfg.on && on {
            scene.piles = [0.10; 5];
            scene.flakes.clear();
        }
        scene.cfg = Config { on, size: size.clamp(60, 160), lights, garland: garland.min(3), snow, snowfall, intensity: intensity.min(100) };
        if !snow {
            scene.piles = [0.0; 5];
            scene.flakes.clear();
        }
    });
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
                CLASS, w!("Snowfall GPU Christmas tree"), WS_POPUP,
                0, 0, 0, 0, None, None, Some(instance.into()), None
            ) else { return };
            hwnd = created;
            slot.set(hwnd.0 as isize);
            let _ = SetLayeredWindowAttributes(hwnd, CLEAR, 0, LWA_COLORKEY);
            SetTimer(Some(hwnd), TIMER, 66, None);
        }
        let s = SCENE.with_borrow(|scene| scene.cfg.size) as i32;
        let width = 260 * s / 100;
        let height = 346 * s / 100;
        let x = GetSystemMetrics(SM_CXSCREEN) - width - 22;
        let y = GetSystemMetrics(SM_CYSCREEN) - height - 60;
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

fn rand(scene: &mut Scene) -> f32 {
    scene.seed ^= scene.seed << 13;
    scene.seed ^= scene.seed >> 17;
    scene.seed ^= scene.seed << 5;
    scene.seed as f32 / u32::MAX as f32
}

fn animate(scene: &mut Scene) {
    scene.frame = scene.frame.wrapping_add(1);
    if !scene.cfg.on || !scene.cfg.snow { return; }
    let intensity = if scene.cfg.snowfall { scene.cfg.intensity as f32 / 100.0 } else { 0.0 };
    // Actual flakes fall from the top, strike the first branch surface they meet,
    // then add to that branch's snow load. Overloaded branches shed small clumps.
    if intensity > 0.0 {
        let interval = (17.0 - 14.0 * intensity).max(3.0) as u32;
        if scene.frame % interval == 0 && scene.flakes.len() < 90 {
            let x = 17.0 + rand(scene) * 226.0;
            let vx = (rand(scene) - 0.5) * 0.45;
            scene.flakes.push(Flake { x, y: -5.0, vx, vy: 1.6 + intensity, sliding: false });
        }
    }
    let mut hits = [0.0; 5];
    for flake in &mut scene.flakes {
        let before = flake.y;
        flake.vy = (flake.vy + 0.13).min(5.0);
        flake.x += flake.vx;
        flake.y += flake.vy;
        if !flake.sliding {
            for i in 0..LEVELS.len() {
                let top = LEVELS[i] - 10.0;
                if before <= top && flake.y >= top && (flake.x - 130.0).abs() < REACH[i] * 0.82 {
                    hits[i] += 0.05;
                    flake.y = 400.0;
                    break;
                }
            }
        }
    }
    scene.flakes.retain(|f| f.y < 346.0 && f.x > -10.0 && f.x < 270.0);
    for (i, hit) in hits.into_iter().enumerate() {
        scene.piles[i] = (scene.piles[i] + hit).min(1.0);
    }
    for i in 0..LEVELS.len() {
        // Gravity and wind eventually overcome the snow's grip on angled boughs.
        let threshold = 0.62 + 0.045 * i as f32;
        if scene.piles[i] > threshold && scene.frame % (14 + 3 * i as u32) == 0 {
            let side = if rand(scene) > 0.5 { 1.0 } else { -1.0 };
            scene.piles[i] = (scene.piles[i] - 0.16).max(0.05);
            scene.flakes.push(Flake {
                x: 130.0 + side * REACH[i] * 0.76,
                y: LEVELS[i] - 4.0,
                vx: side * (0.8 + rand(scene) * 0.6),
                vy: 0.45,
                sliding: true,
            });
        }
        // Very slow settling even between snowfall bursts, no endless growth.
        if intensity == 0.0 { scene.piles[i] *= 0.9995; }
    }
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_TIMER => {
            SCENE.with_borrow_mut(animate);
            unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
            LRESULT(0)
        }
        WM_PAINT => { paint(hwnd); LRESULT(0) }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}
fn pt(x: f32, y: f32, scale: f32) -> POINT {
    POINT { x: (x * scale) as i32, y: (y * scale) as i32 }
}
fn fill(hdc: HDC, points: &[POINT], color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old = SelectObject(hdc, HGDIOBJ(brush.0));
        let _ = Polygon(hdc, points);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}
fn circle(hdc: HDC, x: f32, y: f32, radius: f32, color: COLORREF, scale: f32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let old = SelectObject(hdc, HGDIOBJ(brush.0));
        let _ = Ellipse(hdc,
            ((x-radius)*scale) as i32, ((y-radius)*scale) as i32,
            ((x+radius)*scale) as i32, ((y+radius)*scale) as i32);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

fn bough(hdc: HDC, level: usize, scale: f32) {
    let (top, width) = (LEVELS[level] - 61.0, REACH[level]);
    // A dense uneven fir silhouette: different-length twigs instead of stacked triangles.
    let mut points = Vec::with_capacity(35);
    points.push(pt(130.0, top, scale));
    for k in 1..=8 {
        let t = k as f32 / 8.0;
        let noise = ((k * 13 + level * 7) % 9) as f32 / 20.0;
        points.push(pt(130.0 - (width * t * (0.82 + noise)), top + t * 62.0, scale));
        points.push(pt(130.0 - width * (t * 0.92), top + t * 62.0 - 5.0, scale));
    }
    points.push(pt(130.0, LEVELS[level] + 3.0, scale));
    for k in (1..=8).rev() {
        let t = k as f32 / 8.0;
        let noise = ((k * 11 + level * 5) % 8) as f32 / 20.0;
        points.push(pt(130.0 + width * t * (0.82 + noise), top + t * 62.0, scale));
        points.push(pt(130.0 + width * (t * 0.92), top + t * 62.0 - 5.0, scale));
    }
    let palette = [
        rgb(15, 80, 48), rgb(17, 97, 54), rgb(16, 93, 50),
        rgb(15, 88, 47), rgb(12, 76, 42),
    ];
    fill(hdc, &points, palette[level]);
    // Blue-green needles, shadowed inner boughs and highlights.
    for needle in 0..50 {
        let k = needle as f32;
        let t = (needle * 37 % 100) as f32 / 100.0;
        let y = top + 12.0 + t * 44.0;
        let x = 130.0 + ((needle * 29 + level * 41) % 193) as f32 / 96.0 * width - width;
        let edge = width * ((y - top) / 62.0).clamp(0.15, 1.0);
        if (x - 130.0).abs() < edge * 0.8 {
            let shade = if needle % 3 == 0 { rgb(33, 128, 71) }
                else if needle % 3 == 1 { rgb(24, 106, 58) } else { rgb(10, 61, 38) };
            let tiny = [
                pt(x - 1.5, y, scale),
                pt(x + 5.5 + (k % 3.0), y - 4.0, scale),
                pt(x + 2.0, y + 3.0, scale),
            ];
            fill(hdc, &tiny, shade);
        }
    }
}

fn paint(hwnd: HWND) {
    let (cfg, frame, piles, flakes) = SCENE.with_borrow(|s| (s.cfg, s.frame, s.piles, s.flakes.clone()));
    let scale = cfg.size as f32 / 100.0;
    let p = |v: f32| (v * scale) as i32;
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let bg = CreateSolidBrush(CLEAR);
        let rect = RECT { left: 0, top: 0, right: p(260.0), bottom: p(346.0) };
        FillRect(hdc, &rect, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        // Trunk drawn behind the foliage.
        let trunk = CreateSolidBrush(rgb(108, 66, 39));
        FillRect(hdc, &RECT { left: p(115.0), top: p(253.0), right: p(146.0), bottom: p(322.0) }, trunk);
        let _ = DeleteObject(HGDIOBJ(trunk.0));
        for i in 0..LEVELS.len() { bough(hdc, i, scale); }

        // An illuminated five-point topper.
        let points: Vec<POINT> = (0..10).map(|i| {
            let a = (i as f32 * 36.0 - 90.0) * PI / 180.0;
            let r = if i % 2 == 0 { 18.0 } else { 7.8 };
            pt(130.0 + a.cos() * r, 27.0 + a.sin() * r, scale)
        }).collect();
        fill(hdc, &points, rgb(255, 223, 90));

        // Draped strings of configurable coloured, warm, icy or retro bulbs.
        for (row, &(y, half)) in [(105.0, 44.0), (153.0, 72.0), (201.0, 94.0), (250.0, 108.0)].iter().enumerate() {
            for i in 0..13 {
                let t = i as f32 / 12.0;
                let x = 130.0 + (t * 2.0 - 1.0) * half;
                let yy = y + (1.0 - (t * 2.0 - 1.0).powi(2)) * 11.0;
                // A thin gold string connecting the bulbs.
                if i < 12 {
                    fill(hdc, &[
                        pt(x, yy, scale),
                        pt(x + half * 2.0 / 12.0, yy + (0.5-t) * 3.0 + 1.0, scale),
                        pt(x + half * 2.0 / 12.0, yy + (0.5-t) * 3.0 + 2.0, scale),
                    ], rgb(118, 92, 41));
                }
                let palette = match cfg.garland {
                    1 => [rgb(255, 224, 136), rgb(255, 239, 173), rgb(255, 199, 100), rgb(255, 230, 140)],
                    2 => [rgb(152, 218, 255), rgb(218, 249, 255), rgb(111, 168, 255), rgb(184, 227, 255)],
                    _ => [rgb(255, 75, 68), rgb(255, 222, 74), rgb(102, 191, 255), rgb(221, 125, 250)],
                };
                let bright = !cfg.lights || ((frame / 5 + row as u32 * 3 + i as u32) % 7 != 0);
                let color = if bright { palette[(i + row) % 4] } else { rgb(56, 67, 56) };
                let radius = if cfg.garland == 3 { 5.1 } else { 3.0 };
                circle(hdc, x, yy, radius, color, scale);
            }
        }

        // Snow load sits on particular boughs; falling flakes and edge slides are animated.
        if cfg.snow {
            for i in 0..LEVELS.len() {
                let mass = piles[i];
                if mass < 0.035 { continue; }
                let span = REACH[i] * (0.22 + mass * 0.38);
                let top = LEVELS[i] - 6.0 - 6.0 * mass;
                let snow = CreateSolidBrush(rgb(238, 246, 252));
                let old = SelectObject(hdc, HGDIOBJ(snow.0));
                let _ = Ellipse(hdc, p(130.0-span), p(top), p(130.0+span), p(LEVELS[i]+2.0));
                let _ = SelectObject(hdc, old);
                let _ = DeleteObject(HGDIOBJ(snow.0));
            }
            for flake in &flakes {
                circle(hdc, flake.x, flake.y, if flake.sliding { 3.4 } else { 1.9 }, rgb(246, 250, 255), scale);
            }
        }

        // Snow at the foot of the trunk.
        let snow = CreateSolidBrush(rgb(236, 245, 255));
        let old = SelectObject(hdc, HGDIOBJ(snow.0));
        let _ = Ellipse(hdc, p(38.0), p(310.0), p(225.0), p(339.0));
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(snow.0));
        EndPaint(hwnd, &ps);
    }
}
