use std::collections::HashMap;
use std::fs;
use std::ptr;
use std::sync::Arc;
use fontdue::{Font, FontSettings, Metrics};
use windows::core::Interface;
use tiny_skia::{Color, FillRule, Mask, Pixmap, PixmapPaint, Transform};
use windows::Win32::Foundation::{BOOL, HMODULE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_WRITE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DYNAMIC,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGIOutput, IDXGISwapChain1, DXGI_PRESENT,
    DXGI_PRESENT_PARAMETERS, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC, HBITMAP,
    BLENDFUNCTION, AC_SRC_ALPHA, AC_SRC_OVER,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, UpdateLayeredWindow, GWL_EXSTYLE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, ULW_ALPHA,
    WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP,
};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct GlyphKey {
    ch: char,
    size_bits: u32,
    bold: bool,
    icon: bool,
}

struct CachedGlyph {
    metrics: Metrics,
    bitmap: Vec<u8>,
}

struct HardwareHandles {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    upload: ID3D11Texture2D,
    dcomp_device: IDCompositionDevice,
    dcomp_target: IDCompositionTarget,
    dcomp_visual: IDCompositionVisual,
}

pub struct LayeredCanvas {
    pub width: i32,
    pub height: i32,
    hdc_mem: HDC,
    hbitmap: HBITMAP,
    dib_ptr: *mut u8,
    pub pixmap: Pixmap,
    pub content_pixmap: Pixmap,
    pub mask: Mask,
    // Hardware-accelerated presentation path (D3D11 -> DXGI -> DirectComposition).
    // All `None` when the CPU/UpdateLayeredWindow fallback is in use.
    d3d_device: Option<ID3D11Device>,
    d3d_context: Option<ID3D11DeviceContext>,
    swapchain: Option<IDXGISwapChain1>,
    upload_texture: Option<ID3D11Texture2D>,
    _dcomp_device: Option<IDCompositionDevice>,
    _dcomp_target: Option<IDCompositionTarget>,
    _dcomp_visual: Option<IDCompositionVisual>,
    hardware_ready: bool,
    pub font_regular: Option<Arc<Font>>,
    pub font_bold: Option<Arc<Font>>,
    pub font_icon: Option<Arc<Font>>,
    glyph_cache: HashMap<GlyphKey, CachedGlyph>,
}

impl LayeredCanvas {
    pub fn new(width: i32, height: i32, hwnd: HWND) -> windows::core::Result<Self> {
        unsafe {
            let hdc_mem = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: (width * height * 4) as u32,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
            };

            let mut bits: *mut std::ffi::c_void = ptr::null_mut();
            let hbitmap = CreateDIBSection(
                hdc_mem,
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            )?;

            SelectObject(hdc_mem, hbitmap);

            let pixmap = Pixmap::new(width as u32, height as u32)
                .expect("Failed to allocate tiny-skia pixmap");
            let content_pixmap = Pixmap::new(width as u32, height as u32)
                .expect("Failed to allocate content pixmap");
            let mask = Mask::new(width as u32, height as u32)
                .expect("Failed to allocate mask");

            // Load Segoe UI fonts from Windows Fonts directory dynamically
            let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            let fonts_dir = std::path::PathBuf::from(&system_root).join("Fonts");
            let reg_path = fonts_dir.join("segoeui.ttf");
            let reg_bytes = fs::read(&reg_path);
            let font_regular = reg_bytes
                .as_ref()
                .ok()
                .and_then(|data| Font::from_bytes(&data[..], FontSettings::default()).ok())
                .map(Arc::new);

            let bold_path = fonts_dir.join("segoeuib.ttf");
            let bold_bytes = fs::read(&bold_path);
            let font_bold = bold_bytes
                .as_ref()
                .ok()
                .and_then(|data| Font::from_bytes(&data[..], FontSettings::default()).ok())
                .map(Arc::new)
                .or_else(|| font_regular.clone());

            // Load standard Windows icons (Segoe MDL2 / Fluent)
            let icon_path = fonts_dir.join("segmdl2.ttf");
            let icon_bytes = fs::read(&icon_path);
            let font_icon = icon_bytes
                .as_ref()
                .ok()
                .and_then(|data| Font::from_bytes(&data[..], FontSettings::default()).ok())
                .map(Arc::new);

            // Pre-warm glyph cache for instant, zero-stutter first-frame rendering
            let mut glyph_cache = HashMap::with_capacity(512);
            let common_sizes = [9.5f32, 11.5, 12.0, 12.5, 13.0, 13.5, 14.0];
            let prewarm_chars = "0123456789:\u{2022}-\u{2014}.,!/?% APMabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
            for &size in &common_sizes {
                if let Some(ref f) = font_regular {
                    for ch in prewarm_chars.chars() {
                        let (metrics, bitmap) = f.rasterize(ch, size);
                        glyph_cache.insert(GlyphKey { ch, size_bits: size.to_bits(), bold: false, icon: false }, CachedGlyph { metrics, bitmap });
                    }
                }
                if let Some(ref f) = font_bold {
                    for ch in prewarm_chars.chars() {
                        let (metrics, bitmap) = f.rasterize(ch, size);
                        glyph_cache.insert(GlyphKey { ch, size_bits: size.to_bits(), bold: true, icon: false }, CachedGlyph { metrics, bitmap });
                    }
                }
            }

            let hardware = if std::env::var_os("MYISLAND_DISABLE_GPU").is_some() {
                None
            } else {
                Self::try_init_hardware(width, height, hwnd)
            };
            if hardware.is_some() {
                Self::enable_composition_style(hwnd);
            } else {
                Self::restore_layered_style(hwnd);
            }

            Ok(Self {
                width,
                height,
                hdc_mem,
                hbitmap,
                dib_ptr: bits as *mut u8,
                pixmap,
                content_pixmap,
                mask,
                d3d_device: hardware.as_ref().map(|h| h.device.clone()),
                d3d_context: hardware.as_ref().map(|h| h.context.clone()),
                swapchain: hardware.as_ref().map(|h| h.swapchain.clone()),
                upload_texture: hardware.as_ref().map(|h| h.upload.clone()),
                _dcomp_device: hardware.as_ref().map(|h| h.dcomp_device.clone()),
                _dcomp_target: hardware.as_ref().map(|h| h.dcomp_target.clone()),
                _dcomp_visual: hardware.as_ref().map(|h| h.dcomp_visual.clone()),
                hardware_ready: hardware.is_some(),
                font_regular,
                font_bold,
                font_icon,
                glyph_cache,
            })
        }
    }

    /// Attempt to bring up the hardware presentation stack. Any failure at any
    /// stage simply returns `None`, leaving the DIB/UpdateLayeredWindow path in
    /// charge. This keeps the app functional on machines without a usable
    /// hardware D3D11 adapter (RDP, VMs, old drivers, ...).
    fn try_init_hardware(width: i32, height: i32, hwnd: HWND) -> Option<HardwareHandles> {
        macro_rules! fail {
            () => {{
                return None;
            }};
        }
        macro_rules! try_result {
            ($expr:expr) => {
                match $expr {
                    Ok(value) => value,
                    Err(_) => fail!(),
                }
            };
        }
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;

            try_result!(D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            ));

            let device = match device {
                Some(device) => device,
                None => fail!(),
            };
            let context = match context {
                Some(context) => context,
                None => fail!(),
            };

            let dxgi_device: IDXGIDevice = try_result!(device.cast());
            let adapter = try_result!(dxgi_device.GetAdapter());
            let factory: IDXGIFactory2 = try_result!(adapter.GetParent());

            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width as u32,
                Height: height as u32,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: BOOL(0),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: 0,
            };

            let swapchain = try_result!(factory.CreateSwapChainForComposition(
                &dxgi_device,
                &desc,
                None::<&IDXGIOutput>,
            ));

            // A single reusable dynamic texture is the CPU -> GPU upload
            // target. Keeping it dynamic lets each frame Map/WRITE_DISCARD it
            // without allocating a new texture.
            let upload_desc = D3D11_TEXTURE2D_DESC {
                Width: width as u32,
                Height: height as u32,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DYNAMIC,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                MiscFlags: 0,
            };

            let mut upload: Option<ID3D11Texture2D> = None;
            try_result!(device.CreateTexture2D(&upload_desc, None, Some(&mut upload)));
            let upload = match upload {
                Some(upload) => upload,
                None => fail!(),
            };

            let dcomp_device: IDCompositionDevice =
                try_result!(DCompositionCreateDevice(&dxgi_device));
            let dcomp_target =
                try_result!(dcomp_device.CreateTargetForHwnd(hwnd, BOOL(1)));
            let dcomp_visual = try_result!(dcomp_device.CreateVisual());

            try_result!(dcomp_visual.SetContent(&swapchain));
            try_result!(dcomp_target.SetRoot(&dcomp_visual));
            try_result!(dcomp_device.Commit());

            Some(HardwareHandles {
                device,
                context,
                swapchain,
                upload,
                dcomp_device,
                dcomp_target,
                dcomp_visual,
            })
        }
    }

    /// DirectComposition presents through the DWM composition tree, so the
    /// window must stop using the legacy layered redirection surface.
    fn enable_composition_style(hwnd: HWND) {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let style = (style & !(WS_EX_LAYERED.0 as isize))
                | WS_EX_NOREDIRECTIONBITMAP.0 as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }

    /// Restore the layered-window style when the GPU path cannot present so
    /// the CPU fallback keeps producing a visible alpha window.
    fn restore_layered_style(hwnd: HWND) {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let style = (style & !(WS_EX_NOREDIRECTIONBITMAP.0 as isize))
                | WS_EX_LAYERED.0 as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }

    /// Drop the GPU objects and return to the DIB/UpdateLayeredWindow path.
    fn disable_hardware(&mut self, hwnd: HWND) {
        self.hardware_ready = false;
        self.d3d_device = None;
        self.d3d_context = None;
        self.swapchain = None;
        self.upload_texture = None;
        self._dcomp_device = None;
        self._dcomp_target = None;
        self._dcomp_visual = None;

        Self::restore_layered_style(hwnd);
    }

    #[allow(dead_code)]
    pub fn is_hardware_accelerated(&self) -> bool {
        self.hardware_ready
    }

    pub fn clear(&mut self) {
        self.pixmap.fill(Color::TRANSPARENT);
    }

    /// Begin rendering island content with overflow:hidden clipping.
    /// Directs drawing operations to an offscreen transparent pixmap.
    pub fn begin_island_content(&mut self) {
        self.content_pixmap.fill(Color::TRANSPARENT);
        std::mem::swap(&mut self.pixmap, &mut self.content_pixmap);
    }

    /// Complete island content rendering, apply the exact squircle mask (overflow: hidden),
    /// and composite onto the main surface with an optional centered content transform.
    pub fn end_island_content(
        &mut self,
        path: Option<&tiny_skia::Path>,
        content_scale: f32,
        center_x: f32,
        center_y: f32,
    ) {
        std::mem::swap(&mut self.pixmap, &mut self.content_pixmap);
        if let Some(p) = path {
            self.mask.clear();
            self.mask.fill_path(p, FillRule::Winding, true, Transform::identity());
            self.content_pixmap.apply_mask(&self.mask);
        }
        let paint = PixmapPaint::default();
        let mut transform = Transform::from_translate(-center_x, -center_y);
        transform = transform.post_scale(content_scale, content_scale);
        transform = transform.post_translate(center_x, center_y);
        self.pixmap.draw_pixmap(
            0,
            0,
            self.content_pixmap.as_ref(),
            &paint,
            transform,
            None,
        );
    }

    /// Convenience wrapper for unscaled content compositing.
    #[allow(dead_code)]
    pub fn end_island_content_plain(&mut self, path: Option<&tiny_skia::Path>) {
        self.end_island_content(path, 1.0, 0.0, 0.0);
    }

    /// Whether presentation is currently going through the D3D11/DXGI/
    /// DirectComposition path rather than the `UpdateLayeredWindow` fallback.
    pub fn uses_hardware(&self) -> bool {
        self.hardware_ready
    }

    /// Present the pixmap to the Win32 layered window.
    pub fn present(&mut self, hwnd: HWND, window_x: i32, window_y: i32) {
        let src_data = self.pixmap.data();
        let _dib_slice = unsafe {
            std::slice::from_raw_parts_mut(self.dib_ptr, (self.width * self.height * 4) as usize)
        };

        // Convert RGBA premultiplied to BGRA premultiplied using fast 32-bit SIMD-vectorizable bit manipulation
        let total_pixels = (self.width * self.height) as usize;
        let src_pixels = unsafe {
            std::slice::from_raw_parts(src_data.as_ptr() as *const u32, total_pixels)
        };
        let dst_pixels = unsafe {
            std::slice::from_raw_parts_mut(self.dib_ptr as *mut u32, total_pixels)
        };

        for (src, dst) in src_pixels.iter().zip(dst_pixels.iter_mut()) {
            let px = *src;
            // In little-endian memory:
            // Input RGBA: R = bits 0..7, G = bits 8..15, B = bits 16..23, A = bits 24..31
            // Target BGRA: B = bits 0..7, G = bits 8..15, R = bits 16..23, A = bits 24..31
            *dst = (px & 0xFF00FF00) | ((px & 0x00FF0000) >> 16) | ((px & 0x000000FF) << 16);
        }

        let pt_dst = POINT { x: window_x, y: window_y };
        let size = SIZE { cx: self.width, cy: self.height };
        let pt_src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                None,
                Some(&pt_dst),
                Some(&size),
                self.hdc_mem,
                Some(&pt_src),
                None,
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    /// Present the current pixmap. Prefers the D3D11/DXGI/DirectComposition
    /// path when available and transparently falls back to
    /// `UpdateLayeredWindow` otherwise.
    ///
    /// `dirty_rect` is the accumulated damage for this frame in canvas pixels.
    /// When omitted the whole surface is treated as damaged.
    pub fn present_with_dirty_rect(
        &mut self,
        hwnd: HWND,
        window_x: i32,
        window_y: i32,
        dirty_rect: Option<RECT>,
    ) {
        if self.hardware_ready {
            if self.present_hardware(dirty_rect) {
                return;
            }
            // Device removed/lost or the swapchain rejected the frame. Drop
            // the GPU objects and let the DIB path take over from here.
            self.disable_hardware(hwnd);
        }
        self.present(hwnd, window_x, window_y);
    }

    fn present_hardware(&mut self, _dirty_rect: Option<RECT>) -> bool {
        let (Some(context), Some(swapchain), Some(upload)) = (
            self.d3d_context.as_ref(),
            self.swapchain.as_ref(),
            self.upload_texture.as_ref(),
        ) else {
            return false;
        };

        let width = self.width as usize;
        let height = self.height as usize;
        if width == 0 || height == 0 {
            return true;
        }

        unsafe {
            let backbuffer: ID3D11Texture2D = match swapchain.GetBuffer(0) {
                Ok(buffer) => buffer,
                Err(_) => return false,
            };

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if context
                .Map(
                    upload,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .is_err()
            {
                return false;
            }

            // tiny-skia produces premultiplied RGBA; the swapchain expects
            // premultiplied BGRA. Convert the full surface so backbuffer flips
            // stay completely clean without ghosting, stale buffers, or compositor tearing.
            let src = self.pixmap.data();
            let src_pixels =
                std::slice::from_raw_parts(src.as_ptr() as *const u32, width * height);
            let dst_base = mapped.pData as *mut u8;
            let row_pitch = mapped.RowPitch as usize;

            for y in 0..height {
                let src_row = &src_pixels[y * width..(y + 1) * width];
                let dst_row = std::slice::from_raw_parts_mut(
                    dst_base.add(y * row_pitch) as *mut u32,
                    width,
                );
                for (src_px, dst_px) in src_row.iter().zip(dst_row.iter_mut()) {
                    let px = *src_px;
                    *dst_px = (px & 0xFF00FF00)
                        | ((px & 0x00FF0000) >> 16)
                        | ((px & 0x000000FF) << 16);
                }
            }

            context.Unmap(upload, 0);

            context.CopyResource(&backbuffer, upload);

            let present_params = DXGI_PRESENT_PARAMETERS::default();
            swapchain.Present1(1, DXGI_PRESENT(0), &present_params).is_ok()
        }
    }

    pub fn draw_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
    ) {
        self.draw_text_clipped(text, x, y, size, color, bold, 0.0, self.width as f32);
    }

    pub fn draw_text_clipped(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
        min_x: f32,
        max_x: f32,
    ) {
        self.draw_text_clipped_box(text, x, y, size, color, bold, min_x, max_x, 0.0, self.height as f32);
    }

    pub fn draw_text_clipped_box(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        bold: bool,
        min_x: f32,
        max_x: f32,
        min_y: f32,
        max_y: f32,
    ) {
        let font = if bold {
            self.font_bold.as_ref().or(self.font_regular.as_ref())
        } else {
            self.font_regular.as_ref()
        };

        let font = match font {
            Some(f) => f.clone(),
            None => return,
        };

        let mut curr_x = x;
        let pm_w = self.width as usize;
        let pm_h = self.height as usize;

        for ch in text.chars() {
            let key = GlyphKey {
                ch,
                size_bits: size.to_bits(),
                bold,
                icon: false,
            };

            let cached = self.glyph_cache.entry(key).or_insert_with(|| {
                let (metrics, bitmap) = font.rasterize(ch, size);
                CachedGlyph { metrics, bitmap }
            });

            let metrics = cached.metrics;
            let gx = curr_x + metrics.xmin as f32;
            let gy = y + (size - metrics.ymin as f32 - metrics.height as f32);

            let bitmap = &cached.bitmap;
            let pm_buf = self.pixmap.data_mut();

            for row in 0..metrics.height {
                let py_f = gy + row as f32;
                if py_f < min_y || py_f >= max_y {
                    continue;
                }
                let py = py_f as usize;
                if py >= pm_h {
                    continue;
                }
                for col in 0..metrics.width {
                    let px_f = gx + col as f32;
                    if px_f < min_x || px_f >= max_x {
                        continue;
                    }
                    let px = px_f as usize;
                    if px >= pm_w {
                        continue;
                    }
                    let alpha_val = bitmap[row * metrics.width + col];
                    if alpha_val == 0 {
                        continue;
                    }

                    let a_norm = (alpha_val as f32 / 255.0) * color.alpha();
                    let idx = (py * pm_w + px) * 4;

                    // Premultiplied alpha composite
                    let r_pre = (color.red() * 255.0 * a_norm) as u32;
                    let g_pre = (color.green() * 255.0 * a_norm) as u32;
                    let b_pre = (color.blue() * 255.0 * a_norm) as u32;
                    let a_pre = (a_norm * 255.0) as u32;

                    let dst_r = pm_buf[idx] as u32;
                    let dst_g = pm_buf[idx + 1] as u32;
                    let dst_b = pm_buf[idx + 2] as u32;
                    let dst_a = pm_buf[idx + 3] as u32;

                    let inv_a = 255 - a_pre;
                    pm_buf[idx] = (r_pre + (dst_r * inv_a) / 255).min(255) as u8;
                    pm_buf[idx + 1] = (g_pre + (dst_g * inv_a) / 255).min(255) as u8;
                    pm_buf[idx + 2] = (b_pre + (dst_b * inv_a) / 255).min(255) as u8;
                    pm_buf[idx + 3] = (a_pre + (dst_a * inv_a) / 255).min(255) as u8;
                }
            }

            curr_x += metrics.advance_width;
        }
    }

    pub fn draw_icon_centered(
        &mut self,
        ch: char,
        cx: f32,
        cy: f32,
        size: f32,
        color: Color,
    ) {
        let font = match self.font_icon.as_ref() {
            Some(f) => f.clone(),
            None => return,
        };

        let key = GlyphKey {
            ch,
            size_bits: size.to_bits(),
            bold: false,
            icon: true,
        };

        let cached = self.glyph_cache.entry(key).or_insert_with(|| {
            let (metrics, bitmap) = font.rasterize(ch, size);
            CachedGlyph { metrics, bitmap }
        });

        let metrics = cached.metrics;
        let gx = cx - (metrics.width as f32) / 2.0;
        let gy = cy - (metrics.height as f32) / 2.0;

        let bitmap = &cached.bitmap;
        let pm_buf = self.pixmap.data_mut();
        let pm_w = self.width as usize;
        let pm_h = self.height as usize;

        for row in 0..metrics.height {
            let py_f = gy + row as f32;
            if py_f < 0.0 || py_f >= pm_h as f32 {
                continue;
            }
            let py = py_f as usize;
            for col in 0..metrics.width {
                let px_f = gx + col as f32;
                if px_f < 0.0 || px_f >= pm_w as f32 {
                    continue;
                }
                let px = px_f as usize;
                let alpha_val = bitmap[row * metrics.width + col];
                if alpha_val == 0 {
                    continue;
                }

                let a_norm = (alpha_val as f32 / 255.0) * color.alpha();
                let idx = (py * pm_w + px) * 4;

                let r_pre = (color.red() * 255.0 * a_norm) as u32;
                let g_pre = (color.green() * 255.0 * a_norm) as u32;
                let b_pre = (color.blue() * 255.0 * a_norm) as u32;
                let a_pre = (a_norm * 255.0) as u32;

                let dst_r = pm_buf[idx] as u32;
                let dst_g = pm_buf[idx + 1] as u32;
                let dst_b = pm_buf[idx + 2] as u32;
                let dst_a = pm_buf[idx + 3] as u32;

                let inv_a = 255 - a_pre;
                pm_buf[idx] = (r_pre + (dst_r * inv_a) / 255).min(255) as u8;
                pm_buf[idx + 1] = (g_pre + (dst_g * inv_a) / 255).min(255) as u8;
                pm_buf[idx + 2] = (b_pre + (dst_b * inv_a) / 255).min(255) as u8;
                pm_buf[idx + 3] = (a_pre + (dst_a * inv_a) / 255).min(255) as u8;
            }
        }
    }

    pub fn measure_text_width(&self, text: &str, size: f32, bold: bool) -> f32 {
        let font = if bold {
            self.font_bold.as_ref().or(self.font_regular.as_ref())
        } else {
            self.font_regular.as_ref()
        };

        match font {
            Some(f) => {
                let mut width = 0.0;
                for ch in text.chars() {
                    let metrics = f.metrics(ch, size);
                    width += metrics.advance_width;
                }
                width
            }
            None => text.len() as f32 * (size * 0.55),
        }
    }

    /// Returns the `y` value to pass to `draw_text` so the glyph ink is
    /// vertically centered on `center_y`. `draw_text` positions glyphs from
    /// the top of the font line box, so text of different sizes would
    /// otherwise sit on different optical centerlines.
    pub fn text_y_for_vertical_center(
        &self,
        text: &str,
        size: f32,
        bold: bool,
        center_y: f32,
    ) -> f32 {
        let font = if bold {
            self.font_bold.as_ref().or(self.font_regular.as_ref())
        } else {
            self.font_regular.as_ref()
        };
        let Some(font) = font else {
            return center_y - size * 0.5;
        };

        let mut top = f32::INFINITY;
        let mut bottom = f32::NEG_INFINITY;
        for ch in text.chars() {
            let metrics = font.metrics(ch, size);
            if metrics.height == 0 {
                continue;
            }
            let glyph_top = size - metrics.ymin as f32 - metrics.height as f32;
            let glyph_bottom = size - metrics.ymin as f32;
            top = top.min(glyph_top);
            bottom = bottom.max(glyph_bottom);
        }

        if top.is_finite() && bottom.is_finite() {
            center_y - (top + bottom) * 0.5
        } else {
            center_y - size * 0.5
        }
    }

    pub fn truncate_to_width(
        &self,
        text: &str,
        max_width: f32,
        size: f32,
        bold: bool,
    ) -> String {
        if max_width <= 8.0 {
            return String::new();
        }
        if self.measure_text_width(text, size, bold) <= max_width {
            return text.to_string();
        }

        let ell_w = self.measure_text_width("…", size, bold);
        let avail_w = max_width - ell_w;
        if avail_w <= 0.0 {
            return if max_width >= ell_w { "…".to_string() } else { String::new() };
        }

        let font = if bold {
            self.font_bold.as_ref().or(self.font_regular.as_ref())
        } else {
            self.font_regular.as_ref()
        };

        if let Some(f) = font {
            let mut acc_w = 0.0;
            let mut end_idx = 0;
            for (idx, ch) in text.char_indices() {
                let m = f.metrics(ch, size);
                if acc_w + m.advance_width > avail_w {
                    break;
                }
                acc_w += m.advance_width;
                end_idx = idx + ch.len_utf8();
            }
            if end_idx > 0 {
                let mut res = text[..end_idx].to_string();
                res.push('…');
                return res;
            }
        }

        if max_width >= 12.0 {
            "…".to_string()
        } else {
            String::new()
        }
    }
}

#[allow(dead_code)]
fn clamp_rect(rect: RECT, width: i32, height: i32) -> RECT {
    RECT {
        left: rect.left.clamp(0, width),
        top: rect.top.clamp(0, height),
        right: rect.right.clamp(0, width),
        bottom: rect.bottom.clamp(0, height),
    }
}

impl Drop for LayeredCanvas {
    fn drop(&mut self) {
        unsafe {
            if !self.hbitmap.is_invalid() {
                let _ = DeleteObject(self.hbitmap);
            }
            if !self.hdc_mem.is_invalid() {
                let _ = DeleteDC(self.hdc_mem);
            }
        }
    }
}
