//! A richer click-through evergreen decoration, with tiny independent snow physics.
//! The falling snow in this overlay is approximated locally; the desktop simulation
//! remains unchanged. A 66 ms timer caps redraws when the tree is visible.
use std::cell::{Cell, RefCell};
use std::f32::consts::PI;
use std::mem::size_of;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, Ellipse, EndPaint, FillRect, HDC, HGDIOBJ,
    InvalidateRect, PAINTSTRUCT, Polygon, SelectObject, CreatePen, CreateCompatibleDC, CreateCompatibleBitmap, BitBlt, DeleteDC, MoveToEx, LineTo, HBITMAP, HPEN, PS_SOLID, SRCCOPY,
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
    FIR.with_borrow_mut(|slot| { *slot = None; });
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
            let slide_speed = 0.8 + rand(scene) * 0.6;
            scene.flakes.push(Flake {
                x: 130.0 + side * REACH[i] * 0.76,
                y: LEVELS[i] - 4.0,
                vx: side * slide_speed,
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


struct CachedFir {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    size: u32,
}
thread_local! {
    static FIR: RefCell<Option<CachedFir>> = const { RefCell::new(None) };
}
impl Drop for CachedFir {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}
fn hash(mut v: u32) -> f32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^= v >> 16;
    v as f32 / u32::MAX as f32
}
fn line(hdc: HDC, x1: f32, y1: f32, x2: f32, y2: f32, scale: f32) {
    unsafe {
        let _ = MoveToEx(hdc, (x1 * scale) as i32, (y1 * scale) as i32, None);
        let _ = LineTo(hdc, (x2 * scale) as i32, (y2 * scale) as i32);
    }
}

/// Build a full spruce crown using tapered radial limbs, side shoots and
/// individual needles. Layers overlap rather than forming flat triangles.
/// Rendered once to a small device bitmap; animation only blits the cached tree.
fn draw_fir(hdc: HDC, scale: f32) {
    unsafe {
        // Visible textured trunk, very narrow at the crown.
        let bark = CreateSolidBrush(rgb(81, 48, 31));
        let _ = FillRect(hdc, &RECT {
            left: (125.0 * scale) as i32, top: (40.0 * scale) as i32,
            right: (137.0 * scale) as i32, bottom: (318.0 * scale) as i32,
        }, bark);
        let _ = DeleteObject(HGDIOBJ(bark.0));

        let needles = [
            rgb(8, 46, 36), rgb(12, 57, 41), rgb(13, 68, 44),
            rgb(17, 76, 47), rgb(18, 85, 51), rgb(22, 94, 56),
            rgb(30, 105, 62), rgb(38, 113, 66), rgb(42, 125, 74),
            rgb(49, 132, 78), rgb(62, 145, 90), rgb(83, 160, 106),
        ];
        let pens: Vec<HPEN> = needles.iter()
            .map(|&c| CreatePen(PS_SOLID, (scale * 0.85).round().max(1.0) as i32, c).unwrap_or_default())
            .collect();
        // Back-facing limbs first, front-facing limbs last.
        // The ring radii expand continuously to create a natural silhouette.
        for layer in 0..67u32 {
            let u = layer as f32 / 66.0;
            let y0 = 47.0 + 248.0 * u;
            let radius = 7.5 + 112.0 * u.powf(1.12);
            let branch_count = (9.0 + 13.0 * u) as u32;
            for rank in 0..branch_count {
                let theta = 2.0 * PI * (rank as f32 / branch_count as f32 + layer as f32 * 0.087);
                let depth = theta.sin();
                // Sort by depth by visiting back first then front.
                // This slight per-layer alternation breaks radial symmetry.
                let order_theta = if rank % 2 == 0 { theta + PI } else { theta };
                let dx = order_theta.cos();
                let dz = order_theta.sin();
                let random = hash(layer * 301 + rank * 719 + 17);
                let reach = radius * (0.78 + 0.33 * random);
                let downward = (9.0 + 23.0 * u) * (0.78 + random * 0.3);
                let side = dx * reach;
                let tip_x = 131.0 + side;
                let tip_y = y0 + downward + dz * reach * 0.12;
                let center_x = 131.0 + dz * reach * 0.08;
                let center_y = y0;
                // Main limb under its twigs.
                let dark = if dz > 0.0 { 3 } else { 1 };
                let _ = SelectObject(hdc, HGDIOBJ(pens[dark].0));
                line(hdc, center_x, center_y, tip_x, tip_y, scale);
                let tufts = (7.0 + u * 10.0) as u32;
                for t in 1..=tufts {
                    let along = t as f32 / tufts as f32;
                    let px = center_x + side * along;
                    let py = center_y + (tip_y - center_y) * along;
                    let branch_n = 5.5 + 18.0 * u * (1.0 - along * 0.65);
                    let wobble = hash(layer * 1699 + rank * 463 + t * 101);
                    let tip_width = branch_n * (0.74 + wobble * 0.65);
                    // Left and right side shoots create the soft feathered edges.
                    for direction in [-1.0f32, 1.0] {
                        let tx = px + direction * tip_width * (0.55 + 0.45 * dx.abs());
                        let ty = py - 3.0 - tip_width * 0.35 + dz * 3.0;
                        let shade = if dz >= 0.0 {
                            5 + (hash(layer*97 + rank*53 + t*29 + if direction > 0.0 {2} else {1}) * 7.0) as usize
                        } else {
                            1 + (hash(layer*173 + rank*31 + t*43) * 6.0) as usize
                        };
                        let shade = shade.min(pens.len()-1);
                        let _ = SelectObject(hdc, HGDIOBJ(pens[shade].0));
                        line(hdc, px, py, tx, ty, scale);
                        // Four pairs of needles along each shoot.
                        for needle in 1..=4u32 {
                            let v = needle as f32 / 5.0;
                            let nx = px + (tx-px)*v;
                            let ny = py + (ty-py)*v;
                            let spike = 2.5 + 4.0 * u + 1.4 * hash(layer*233 + rank*97 + t*13 + needle);
                            let _ = SelectObject(hdc, HGDIOBJ(pens[(shade+1).min(11)].0));
                            line(hdc, nx - spike*0.5, ny + spike*0.28, nx + spike*0.25, ny - spike, scale);
                            line(hdc, nx + spike*0.5, ny + spike*0.20, nx - spike*0.25, ny - spike*0.76, scale);
                        }
                    }
                }
                if depth > 0.45 && layer % 4 == 0 {
                    // Lit tips on the front of the crown are pale green.
                    let _ = SelectObject(hdc, HGDIOBJ(pens[10].0));
                    line(hdc, tip_x - 3.0, tip_y - 2.0, tip_x + 3.0, tip_y - 5.0, scale);
                }
            }
        }
        for pen in pens {
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }
    }
}

fn cached_fir(hdc: HDC, size: u32) {
    FIR.with_borrow_mut(|slot| {
        if slot.as_ref().is_none_or(|cache| cache.size != size) {
            *slot = None;
            unsafe {
                let width = 260 * size as i32 / 100;
                let height = 346 * size as i32 / 100;
                let Ok(dc) = CreateCompatibleDC(Some(hdc)) else { return };
                let Ok(bitmap) = CreateCompatibleBitmap(hdc, width, height) else {
                    let _ = DeleteDC(dc);
                    return;
                };
                let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
                let bg = CreateSolidBrush(CLEAR);
                let _ = FillRect(dc, &RECT { left: 0, top: 0, right: width, bottom: height }, bg);
                let _ = DeleteObject(HGDIOBJ(bg.0));
                draw_fir(dc, size as f32 / 100.0);
                *slot = Some(CachedFir { dc, bitmap, previous, size });
            }
        }
        if let Some(cache) = slot.as_ref() {
            unsafe {
                let _ = BitBlt(hdc, 0, 0, 260 * size as i32 / 100,
                    346 * size as i32 / 100, Some(cache.dc), 0, 0, SRCCOPY);
            }
        }
    });
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
        cached_fir(hdc, cfg.size);

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

        // Small irregular snow patches rest on needles, not giant white disks.
        if cfg.snow {
            for i in 0..LEVELS.len() {
                let mass = piles[i];
                if mass < 0.035 { continue; }
                let width = REACH[i] * (0.16 + mass * 0.48);
                let y = LEVELS[i] - 11.0;
                for n in 0..14 {
                    let t = n as f32 / 13.0;
                    let x = 130.0 + (t * 2.0 - 1.0) * width;
                    let chance = hash(i as u32 * 107 + n as u32 * 31 + 29);
                    if chance > mass + 0.15 { continue; }
                    let yy = y + 4.0 * (t - 0.5).abs() + hash(i as u32 * 143 + n as u32 * 17) * 5.0;
                    circle(hdc, x, yy, 1.0 + mass * 1.8, rgb(233, 245, 250), scale);
                }
            }
            for flake in &flakes {
                circle(hdc, flake.x, flake.y, if flake.sliding { 3.1 } else { 1.8 }, rgb(246, 250, 255), scale);
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
