//! Photographic fir with click-through garlands and branch-aware falling snow.
//! The evergreen image is cached as a GDI bitmap; only ornaments and small
//! simulated snowflakes are redrawn. No network or external assets at runtime.
use std::cell::{Cell, RefCell};
use std::f32::consts::PI;
use std::mem::size_of;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, DIB_RGB_COLORS, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
    Ellipse, EndPaint, FillRect, HBITMAP, HDC, HGDIOBJ, InvalidateRect, MoveToEx,
    LineTo, PAINTSTRUCT, Polygon, PS_SOLID, SelectObject, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

const CLASS: PCWSTR = w!("SnowfallGPUChristmasTree");
const CLEAR: COLORREF = COLORREF(0x0003_0201);
const TIMER: usize = 1;
const WIDTH: usize = 260;
const HEIGHT: usize = 346;
const BANDS: usize = 5;
const CELLS: usize = 12;

#[derive(Clone, Copy)]
struct Config {
    on: bool, size: u32, lights: bool, garland: u32,
    snow: bool, snowfall: bool, intensity: u32,
}
#[derive(Clone, Copy)]
struct Flake {
    x: f32, y: f32, vx: f32, vy: f32, sliding: bool, pierce: u8,
}
#[derive(Clone, Copy)]
struct Patch {
    x: f32, y: f32, mass: f32,
}
const EMPTY_PATCH: Patch = Patch { x: 0.0, y: 0.0, mass: 0.0 };

struct Scene {
    cfg: Config,
    frame: u32,
    seed: u32,
    flakes: Vec<Flake>,
    // Opaque fir pixels at logical overlay coordinates, from the real artwork.
    mask: Vec<u8>,
    patches: [[Patch; CELLS]; BANDS],
}
thread_local! {
    static WINDOW: Cell<isize> = const { Cell::new(0) };
    static SCENE: RefCell<Scene> = const { RefCell::new(Scene {
        cfg: Config { on: false, size: 100, lights: true, garland: 0, snow: true, snowfall: true, intensity: 40 },
        frame: 0, seed: 0x5EED_1234, flakes: Vec::new(), mask: Vec::new(),
        patches: [[EMPTY_PATCH; CELLS]; BANDS],
    }) };
    static FIR: RefCell<Option<CachedFir>> = const { RefCell::new(None) };
}
struct CachedFir {
    dc: HDC, bitmap: HBITMAP, previous: HGDIOBJ, size: u32,
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
fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

pub fn configure(on: bool, size: u32, lights: bool, garland: u32, snow: bool, snowfall: bool, intensity: u32) {
    SCENE.with_borrow_mut(|scene| {
        if !scene.cfg.on && on {
            scene.flakes.clear();
            scene.patches = [[EMPTY_PATCH; CELLS]; BANDS];
        }
        scene.cfg = Config { on, size: size.clamp(60, 160), lights, garland: garland.min(3),
            snow, snowfall, intensity: intensity.min(100) };
        if !snow {
            scene.flakes.clear();
            scene.patches = [[EMPTY_PATCH; CELLS]; BANDS];
        }
    });
    WINDOW.with(|slot| unsafe {
        let mut hwnd = HWND(slot.get() as _);
        if hwnd.0.is_null() {
            if !on { return; }
            let Ok(instance) = GetModuleHandleW(None) else { return };
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(procedure), hInstance: instance.into(),
                lpszClassName: CLASS, ..Default::default()
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
        let factor = SCENE.with_borrow(|s| s.cfg.size) as i32;
        let width = WIDTH as i32 * factor / 100;
        let height = HEIGHT as i32 * factor / 100;
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
    FIR.with_borrow_mut(|v| *v = None);
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
fn mask_at(mask: &[u8], x: f32, y: f32) -> u8 {
    if x < 0.0 || y < 0.0 || x >= WIDTH as f32 || y >= HEIGHT as f32 { return 0; }
    mask.get(y as usize * WIDTH + x as usize).copied().unwrap_or(0)
}
fn animate(scene: &mut Scene) {
    scene.frame = scene.frame.wrapping_add(1);
    if !scene.cfg.on || !scene.cfg.snow { return; }
    let intensity = if scene.cfg.snowfall { scene.cfg.intensity as f32 / 100.0 } else { 0.0 };
    // Snowflakes do not hit a fake triangle. They collide with the actual
    // alpha silhouette of the photorealistic fir and land where foliage begins.
    if intensity > 0.0 {
        let interval = (10.0 - 7.0 * intensity).max(3.0) as u32;
        if scene.frame % interval == 0 && scene.flakes.len() < 110 {
            let x = 21.0 + rand(scene) * 218.0;
            let vx = (rand(scene) - 0.5) * 0.85;
            let pierce = if rand(scene) > 0.78 { 1 } else { 0 };
            scene.flakes.push(Flake { x, y: -4.0, vx, vy: 1.5 + intensity, sliding: false, pierce });
        }
    }
    let mut collisions = Vec::new();
    let wind = (scene.frame as f32 * 0.037).sin() * 0.09;
    for f in &mut scene.flakes {
        let prev_x = f.x;
        let prev_y = f.y;
        if f.sliding {
            // A loose clump follows an outward-sloping bough to its tip
            // before gravity takes over. It cannot fall from the centre.
            let nx = f.x + f.vx + wind;
            let ny = f.y + 0.30 + f.vx.abs() * 0.15;
            if mask_at(&scene.mask, nx, ny + 3.0) > 85 {
                f.x = nx;
                f.y = ny;
                f.vx += f.vx.signum() * 0.014;
            } else {
                f.x = nx;
                f.y = ny;
                f.sliding = false;
                f.pierce = 255; // continue falling, do not reattach
                f.vy = 0.85;
            }
        } else {
            f.vy = (f.vy + 0.10).min(5.5);
            f.x += f.vx + wind;
            f.y += f.vy;
        }
        if !f.sliding && f.pierce != 255 && mask_at(&scene.mask, f.x, f.y) > 110 && mask_at(&scene.mask, prev_x, prev_y) <= 110 {
            if f.pierce > 0 { f.pierce -= 1; } else {
                collisions.push((f.x, f.y));
                f.y = 500.0;
            }
        }
    }
    scene.flakes.retain(|f| f.y < HEIGHT as f32 && f.x > -5.0 && f.x < WIDTH as f32 + 5.0);
    // Snow is stored per *local branch position*, not as a disk in the center.
    for (x,y) in collisions {
        let band = ((y - 33.0) / 54.0).floor().clamp(0.0, (BANDS - 1) as f32) as usize;
        let cell = (x / WIDTH as f32 * CELLS as f32).floor().clamp(0.0, (CELLS - 1) as f32) as usize;
        let patch = &mut scene.patches[band][cell];
        if patch.mass == 0.0 { patch.x = x; patch.y = y; }
        else {
            patch.x = patch.x * 0.88 + x * 0.12;
            patch.y = patch.y.min(y);
        }
        patch.mass = (patch.mass + 0.20).min(1.1);
    }
    // Gravity gradually sheds overload on the nearest outer branch edge.
    for band in 0..BANDS {
        for cell in 0..CELLS {
            let patch = scene.patches[band][cell];
            if patch.mass > 0.57 && scene.frame % (13 + band as u32 * 5) == 0 {
                let side = if patch.x < 130.0 { -1.0 } else { 1.0 };
                let speed = 0.65 + rand(scene) * 0.65;
                scene.patches[band][cell].mass = (patch.mass - 0.24).max(0.0);
                if scene.flakes.len() < 110 {
                    scene.flakes.push(Flake { x: patch.x + side * 4.0,
                        y: patch.y + 2.0, vx: side * speed, vy: 0.8,
                        sliding: true, pierce: 0 });
                }
            }
        }
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
fn circle(hdc: HDC, x: f32, y: f32, r: f32, color: COLORREF, scale: f32) {
    unsafe {
        let b = CreateSolidBrush(color);
        let old = SelectObject(hdc, HGDIOBJ(b.0));
        let _ = Ellipse(hdc, ((x-r)*scale) as i32, ((y-r)*scale) as i32,
            ((x+r)*scale) as i32, ((y+r)*scale) as i32);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(b.0));
    }
}
fn pt(x: f32, y: f32, scale: f32) -> POINT {
    POINT { x: (x*scale) as i32, y: (y*scale) as i32 }
}
fn polygon(hdc: HDC, pts: &[POINT], color: COLORREF) {
    unsafe {
        let b = CreateSolidBrush(color);
        let old = SelectObject(hdc, HGDIOBJ(b.0));
        let _ = Polygon(hdc, pts);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(b.0));
    }
}

/// Load the approved fir image via the WIC decoder already used by pumpkins.
/// Build a cached 32-bit colour-keyed bitmap. Its alpha is also retained as
/// the physical collision mask for snowflakes.
fn cached_fir(hdc: HDC, size: u32) {
    FIR.with_borrow_mut(|slot| {
        if slot.as_ref().is_none_or(|c| c.size != size) {
            *slot = None;
            let Ok(pic) = crate::halloween::decode(include_bytes!("../assets/christmas/fir.png")) else { return };
            // Collision surface is independent of display scale, using the
            // precise transparent outline of the same evergreen sprite.
            let sample = |x: f32, y: f32| -> [u8;4] {
                let sx = ((x - 29.0) / 202.0 * pic.width as f32) as i32;
                let sy = ((y - 12.0) / 308.0 * pic.height as f32) as i32;
                if sx < 0 || sy < 0 || sx >= pic.width as i32 || sy >= pic.height as i32 {
                    return [0,0,0,0];
                }
                let i = (sy as usize * pic.width as usize + sx as usize) * 4;
                [pic.pixels[i],pic.pixels[i+1],pic.pixels[i+2],pic.pixels[i+3]]
            };
            let mut mask = vec![0u8; WIDTH * HEIGHT];
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    mask[y * WIDTH + x] = sample(x as f32, y as f32)[3];
                }
            }
            SCENE.with_borrow_mut(|scene| scene.mask = mask);
            unsafe {
                let width = WIDTH as i32 * size as i32 / 100;
                let height = HEIGHT as i32 * size as i32 / 100;
                let dc = CreateCompatibleDC(Some(hdc));
                if dc.0.is_null() { return; }
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width, biHeight: -height, biPlanes: 1,
                        biBitCount: 32, biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits = std::ptr::null_mut();
                let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
                    let _ = DeleteDC(dc);
                    return;
                };
                let dest = std::slice::from_raw_parts_mut(bits.cast::<u8>(), width as usize * height as usize * 4);
                let scale = size as f32 / 100.0;
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let px = sample(x as f32 / scale, y as f32 / scale);
                        let o = (y*width as usize + x) * 4;
                        // DIB memory is BGRA; key colour is RGB(1,2,3).
                        if px[3] < 72 {
                            dest[o..o+4].copy_from_slice(&[3,2,1,0]);
                        } else {
                            let a = px[3].max(1) as u32;
                            let r = (px[0] as u32 * 255 / a).min(255) as u8;
                            let g = (px[1] as u32 * 255 / a).min(255) as u8;
                            let b = (px[2] as u32 * 255 / a).min(255) as u8;
                            dest[o..o+4].copy_from_slice(&[b,g,r,0]);
                        }
                    }
                }
                let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
                *slot = Some(CachedFir { dc, bitmap, previous, size });
            }
        }
        if let Some(cache) = slot.as_ref() {
            unsafe {
                let _ = BitBlt(hdc, 0, 0, WIDTH as i32 * size as i32 / 100,
                    HEIGHT as i32 * size as i32 / 100, Some(cache.dc), 0, 0, SRCCOPY);
            }
        }
    });
}
fn garlands(hdc: HDC, cfg: Config, frame: u32, s: f32) {
    // Five subtly sagging strands following the tree's widening profile.
    let lines = [(88.0, 26.0), (133.0, 44.0), (181.0, 68.0), (228.0, 87.0), (270.0, 100.0)];
    for (row, (y, width)) in lines.into_iter().enumerate() {
        let count = if row < 2 { 10 } else { 16 };
        let shade = match cfg.garland {
            1 => rgb(143,113,60), 2 => rgb(65,96,120), _ => rgb(86,90,64),
        };
        unsafe {
            let pen = CreatePen(PS_SOLID, (s*1.4).round().max(1.0) as i32, shade);
            let old = SelectObject(hdc, HGDIOBJ(pen.0));
            let mut prev: Option<(f32,f32)> = None;
            for i in 0..=36 {
                let t = i as f32 / 36.0;
                let x = 130.0 + (2.0*t - 1.0) * width;
                let yy = y + (1.0 - (2.0*t-1.0).powi(2)) * (7.0 + 2.0*row as f32)
                    + (t * PI * 3.0 + row as f32).sin() * 1.0;
                if let Some((xp,yp)) = prev {
                    let _ = MoveToEx(hdc, (xp*s) as i32, (yp*s) as i32, None);
                    let _ = LineTo(hdc, (x*s) as i32, (yy*s) as i32);
                }
                prev = Some((x,yy));
            }
            let _ = SelectObject(hdc, old);
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }
        for i in 0..=count {
            let t = i as f32 / count as f32;
            let x = 130.0 + (2.0*t - 1.0) * width;
            let yy = y + (1.0 - (2.0*t-1.0).powi(2)) * (7.0 + 2.0*row as f32)
                    + (t * PI * 3.0 + row as f32).sin() * 1.0;
            let palette = match cfg.garland {
                1 => [rgb(255,218,130),rgb(255,240,174),rgb(255,194,91),rgb(255,229,163)],
                2 => [rgb(155,228,255),rgb(214,248,255),rgb(95,183,255),rgb(185,239,255)],
                _ => [rgb(255,96,80),rgb(255,214,81),rgb(89,191,255),rgb(221,127,244)],
            };
            let bright = !cfg.lights || (frame/7 + (row*3+i) as u32) % 8 != 0;
            let col = if bright { palette[(row+i)%4] } else { rgb(43,65,56) };
            let r = if cfg.garland == 3 { 4.4 } else { 2.9 };
            circle(hdc,x,yy,r+1.6,rgb(39,73,51),s);
            circle(hdc,x,yy,r,col,s);
            if bright { circle(hdc,x-0.9,yy-0.9,0.85,rgb(255,251,225),s); }
        }
    }
}
fn paint(hwnd: HWND) {
    let (cfg, frame, patches, flakes) = SCENE.with_borrow(|sc| (sc.cfg, sc.frame, sc.patches, sc.flakes.clone()));
    let scale = cfg.size as f32 / 100.0;
    let p = |v: f32| (v*scale) as i32;
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let bg = CreateSolidBrush(CLEAR);
        let _ = FillRect(hdc, &RECT { left:0,top:0,right:p(WIDTH as f32),bottom:p(HEIGHT as f32) }, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        cached_fir(hdc, cfg.size);
        // Only a soft halo from the topper, no giant fake graphic star.
        let points: Vec<POINT> = (0..10).map(|i| {
            let angle = (i as f32 * 36.0 - 90.0) * PI / 180.0;
            let r = if i%2 == 0 { 9.0 } else { 4.1 };
            pt(130.0 + angle.cos()*r, 23.0 + angle.sin()*r, scale)
        }).collect();
        polygon(hdc, &points, rgb(255,220,96));
        garlands(hdc,cfg,frame,scale);
        if cfg.snow {
            // Snow on actual impact points, with tiny irregular windward caps.
            for row in patches {
                for patch in row {
                    if patch.mass < 0.04 { continue; }
                    let width = 1.7 + patch.mass * 6.0;
                    let y = patch.y - 1.5;
                    for n in 0..5 {
                        let t = n as f32 / 4.0;
                        let x = patch.x + (t-0.5) * width * 2.0;
                        let yy = y - (1.0-(2.0*t-1.0).abs())*patch.mass*2.5;
                        circle(hdc,x,yy,0.9 + patch.mass*1.1,
                            if n % 2 == 0 { rgb(238,249,255) } else { rgb(209,232,246) },scale);
                    }
                }
            }
            for f in flakes {
                circle(hdc,f.x,f.y,if f.sliding { 2.6 } else { 1.65 },rgb(246,251,255),scale);
            }
        }
        EndPaint(hwnd,&ps);
    }
}
