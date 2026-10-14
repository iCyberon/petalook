use anyhow::{Result, anyhow};
use core_foundation::base::TCFType;
use core_video::pixel_buffer::CVPixelBuffer;
use gpui::{Modifiers, ScrollDelta, ScrollWheelEvent, TouchPhase, Window, point, px};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    ffi::{CString, c_char, c_void},
    path::Path,
    ptr::NonNull,
};

#[repr(C)]
#[derive(Default)]
struct NativeImage {
    data: *mut u8,
    width: u32,
    height: u32,
    original_width: u32,
    original_height: u32,
}

unsafe extern "C" {
    fn sf_decode_image(path: *const c_char, limit: u32, out: *mut NativeImage) -> bool;
    fn sf_free_image(data: *mut u8);
    fn sf_image_worker_priority(prefetch: bool);
    fn sf_video_open(path: *const c_char) -> *mut c_void;
    fn sf_video_close(handle: *mut c_void);
    fn sf_video_frame(handle: *mut c_void) -> *mut c_void;
    fn sf_video_time(handle: *mut c_void) -> f64;
    fn sf_video_duration(handle: *mut c_void) -> f64;
    fn sf_video_status(handle: *mut c_void) -> i32;
    fn sf_video_playing(handle: *mut c_void) -> bool;
    fn sf_video_play(handle: *mut c_void, playing: bool);
    fn sf_video_seek(handle: *mut c_void, seconds: f64);
    fn sf_video_mute(handle: *mut c_void, muted: bool);
    fn sf_install_gestures(
        view: *mut c_void,
        context: *mut c_void,
        callback: extern "C" fn(*mut c_void, i32, f64),
        scroll: extern "C" fn(*mut c_void, f64, f64, i32, u32),
    ) -> *mut c_void;
    fn sf_remove_gestures(handle: *mut c_void);
    fn sf_set_icon(path: *const c_char);
}

pub fn image_worker_priority(prefetch: bool) {
    unsafe { sf_image_worker_priority(prefetch) }
}

pub struct DecodedImage {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub original: (u32, u32),
}

pub fn decode_image(path: &Path, limit: u32) -> Result<DecodedImage> {
    let path = CString::new(path.as_os_str().as_encoded_bytes())?;
    let mut out = NativeImage::default();
    // ImageIO creates an independent image inside a worker-thread autorelease pool.
    if !unsafe { sf_decode_image(path.as_ptr(), limit, &mut out) } {
        return Err(anyhow!(
            "This image could not be decoded. It may be damaged or use an unsupported format."
        ));
    }
    let pixels = unsafe {
        let bytes =
            std::slice::from_raw_parts(out.data, out.width as usize * out.height as usize * 4)
                .to_vec();
        sf_free_image(out.data);
        bytes
    };
    Ok(DecodedImage {
        pixels,
        width: out.width,
        height: out.height,
        original: (out.original_width, out.original_height),
    })
}

pub struct Video(NonNull<c_void>);

impl Video {
    pub fn open(path: &Path) -> Result<Self> {
        let path = CString::new(path.as_os_str().as_encoded_bytes())?;
        NonNull::new(unsafe { sf_video_open(path.as_ptr()) })
            .map(Self)
            .ok_or_else(|| anyhow!("Could not open this video."))
    }
    pub fn frame(&self) -> Option<CVPixelBuffer> {
        let raw = unsafe { sf_video_frame(self.0.as_ptr()) };
        // The native function returns a +1 retained pixel buffer. GPUI retains its own copy.
        (!raw.is_null()).then(|| unsafe { CVPixelBuffer::wrap_under_create_rule(raw.cast()) })
    }
    pub fn time(&self) -> f64 {
        finite(unsafe { sf_video_time(self.0.as_ptr()) })
    }
    pub fn duration(&self) -> f64 {
        finite(unsafe { sf_video_duration(self.0.as_ptr()) })
    }
    pub fn failed(&self) -> bool {
        unsafe { sf_video_status(self.0.as_ptr()) == 2 }
    }
    pub fn playing(&self) -> bool {
        unsafe { sf_video_playing(self.0.as_ptr()) }
    }
    pub fn play(&self, playing: bool) {
        unsafe { sf_video_play(self.0.as_ptr(), playing) }
    }
    pub fn seek(&self, seconds: f64) {
        unsafe { sf_video_seek(self.0.as_ptr(), seconds.clamp(0., self.duration())) }
    }
    pub fn mute(&self, muted: bool) {
        unsafe { sf_video_mute(self.0.as_ptr(), muted) }
    }
}
impl Drop for Video {
    fn drop(&mut self) {
        unsafe { sf_video_close(self.0.as_ptr()) }
    }
}
fn finite(value: f64) -> f64 {
    if value.is_finite() { value.max(0.) } else { 0. }
}

#[derive(Clone)]
pub enum Gesture {
    Magnify(f32),
    SmartZoom,
    Swipe(f32),
    CancelSwipe,
    TransitionFinished(u64),
    Scroll(ScrollWheelEvent),
}
pub struct Gestures {
    monitor: NonNull<c_void>,
    sender: Box<async_channel::Sender<Gesture>>,
}
impl Gestures {
    pub fn install(window: &Window, sender: async_channel::Sender<Gesture>) -> Option<Self> {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw()
        else {
            return None;
        };
        let mut sender = Box::new(sender);
        let context = (&mut *sender as *mut async_channel::Sender<Gesture>).cast();
        let monitor = NonNull::new(unsafe {
            sf_install_gestures(
                handle.ns_view.as_ptr(),
                context,
                gesture_callback,
                scroll_callback,
            )
        })?;
        Some(Self { monitor, sender })
    }
}
extern "C" fn gesture_callback(context: *mut c_void, kind: i32, value: f64) {
    let sender = unsafe { &*context.cast::<async_channel::Sender<Gesture>>() };
    let event = match kind {
        0 => Gesture::Magnify(value as f32),
        1 => Gesture::SmartZoom,
        2 => Gesture::Swipe(value as f32),
        4 => Gesture::TransitionFinished(value as u64),
        _ => Gesture::CancelSwipe,
    };
    let _ = sender.try_send(event);
}
extern "C" fn scroll_callback(context: *mut c_void, x: f64, y: f64, phase: i32, flags: u32) {
    let sender = unsafe { &*context.cast::<async_channel::Sender<Gesture>>() };
    let event = ScrollWheelEvent {
        delta: if flags & 1 != 0 {
            ScrollDelta::Pixels(point(px(x as f32), px(y as f32)))
        } else {
            ScrollDelta::Lines(point(x as f32, y as f32))
        },
        touch_phase: match phase {
            0 => TouchPhase::Started,
            2 => TouchPhase::Ended,
            _ => TouchPhase::Moved,
        },
        modifiers: Modifiers {
            platform: flags & 2 != 0,
            control: flags & 4 != 0,
            shift: flags & 8 != 0,
            alt: flags & 16 != 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let _ = sender.try_send(Gesture::Scroll(event));
}
impl Drop for Gestures {
    fn drop(&mut self) {
        unsafe { sf_remove_gestures(self.monitor.as_ptr()) };
        self.sender.close();
    }
}
pub fn set_icon(path: &Path) {
    if let Ok(path) = CString::new(path.as_os_str().as_encoded_bytes()) {
        unsafe { sf_set_icon(path.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_scroll_preserves_phase_delta_and_modifiers() {
        let (sender, receiver) = async_channel::unbounded();
        let context = (&sender as *const async_channel::Sender<super::Gesture>)
            .cast_mut()
            .cast();
        for (phase, expected) in [
            (0, gpui::TouchPhase::Started),
            (1, gpui::TouchPhase::Moved),
            (2, gpui::TouchPhase::Ended),
        ] {
            super::scroll_callback(context, -31., 2., phase, 31);
            let super::Gesture::Scroll(event) = receiver.try_recv().unwrap() else {
                panic!("expected scroll");
            };
            assert_eq!(
                std::mem::discriminant(&event.touch_phase),
                std::mem::discriminant(&expected)
            );
            assert_eq!(
                event.delta.pixel_delta(gpui::px(24.)),
                gpui::point(gpui::px(-31.), gpui::px(2.))
            );
            assert!(
                event.modifiers.platform
                    && event.modifiers.control
                    && event.modifiers.shift
                    && event.modifiers.alt
            );
            assert!(receiver.is_empty());
        }
        super::scroll_callback(context, 1., -2., 1, 0);
        let super::Gesture::Scroll(event) = receiver.try_recv().unwrap() else {
            panic!("expected wheel scroll");
        };
        assert!(matches!(event.delta, gpui::ScrollDelta::Lines(_)));
        assert_eq!(
            event.delta.pixel_delta(gpui::px(24.)),
            gpui::point(gpui::px(24.), gpui::px(-48.))
        );
    }
    use super::*;
    #[test]
    fn imageio_limits_decode_size_and_preserves_original_dimensions() {
        let path = crate::asset_path("sunflower.png");
        let decoded = decode_image(&path, 128).unwrap();
        assert!(decoded.width <= 128 && decoded.height <= 128);
        assert!(decoded.original.0 > 1000 && decoded.original.1 > 1000);
        assert_eq!(
            decoded.pixels.len(),
            decoded.width as usize * decoded.height as usize * 4
        );
        assert_eq!(
            decoded.pixels[3], 0,
            "transparent corner must stay transparent"
        );
    }
    #[test]
    fn imageio_rejects_missing_or_nonimage_files() {
        assert!(decode_image(Path::new("/nonexistent/petalook-test.png"), 128).is_err());
        assert!(
            decode_image(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
                128
            )
            .is_err()
        );
    }
    #[test]
    fn imageio_returns_bgra_and_correct_row_order() {
        let path = std::env::temp_dir().join(format!("petalook-pixels-{}.png", std::process::id()));
        image::save_buffer(
            &path,
            &[255, 0, 0, 255, 0, 0, 255, 255],
            1,
            2,
            image::ColorType::Rgba8,
        )
        .unwrap();
        let decoded = decode_image(&path, 16).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(decoded.pixels, [0, 0, 255, 255, 255, 0, 0, 255]);
    }
}

unsafe extern "C" {
    fn sf_preference(key: *const c_char) -> i32;
    fn sf_set_preference(key: *const c_char, value: i32);
    fn sf_saved_size(w: *mut f64, h: *mut f64);
    fn sf_fit_size(w: *mut f64, h: *mut f64);
    fn sf_save_fit_size(w: f64, h: f64);
    fn sf_window_configure(view: *mut c_void);
    fn sf_window_chrome(view: *mut c_void, visible: bool);
    fn sf_window_save(view: *mut c_void);
    fn sf_window_available(view: *mut c_void, w: *mut f64, h: *mut f64);
    fn sf_window_resize(view: *mut c_void, w: f64, h: f64);
    fn sf_window_start_drag(view: *mut c_void);
    fn sf_window_toggle_fullscreen(view: *mut c_void);
    fn sf_window_snapshot(
        view: *mut c_void,
        bytes: *const u8,
        width: u32,
        height: u32,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        direction: i32,
    );
    fn sf_window_snapshot_video(
        view: *mut c_void,
        buffer: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        direction: i32,
    );
    fn sf_window_begin_transition(view: *mut c_void) -> bool;
    fn sf_window_transition(view: *mut c_void, progress: f64);
    fn sf_window_finish_transition(view: *mut c_void);
    fn sf_window_complete_transition(view: *mut c_void, epoch: u64);
    fn sf_window_restore_origin(view: *mut c_void);
    fn sf_reduce_motion() -> bool;
    #[cfg(debug_assertions)]
    fn sf_window_transition_probe(view: *mut c_void, values: *mut f64) -> i32;
    #[cfg(debug_assertions)]
    fn sf_window_snapshot_count(view: *mut c_void) -> i32;
    fn sf_default_status() -> i32;
    fn sf_default_error() -> *const c_char;
    fn sf_make_default();
    fn sf_zoom_menu(mode: i32);
}
fn view_ptr(window: &Window) -> *mut c_void {
    match HasWindowHandle::window_handle(window)
        .expect("native window")
        .as_raw()
    {
        RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr(),
        _ => unreachable!("macOS only"),
    }
}
pub fn preference(key: &str) -> i32 {
    let key = CString::new(key).expect("preference key");
    unsafe { sf_preference(key.as_ptr()) }
}
pub fn set_preference(key: &str, value: i32) {
    let key = CString::new(key).expect("preference key");
    unsafe { sf_set_preference(key.as_ptr(), value) }
}
pub fn saved_size() -> gpui::Size<gpui::Pixels> {
    let (mut w, mut h) = (0., 0.);
    unsafe { sf_saved_size(&mut w, &mut h) };
    gpui::size(gpui::px(w as f32), gpui::px(h as f32))
}
pub fn fit_size() -> crate::geometry::Extent {
    let (mut w, mut h) = (0., 0.);
    unsafe { sf_fit_size(&mut w, &mut h) };
    crate::geometry::Extent {
        width: w as f32,
        height: h as f32,
    }
}
pub fn save_fit_size(size: crate::geometry::Extent) {
    unsafe { sf_save_fit_size(size.width as f64, size.height as f64) }
}
pub fn configure_window(window: &Window) {
    unsafe { sf_window_configure(view_ptr(window)) }
}
pub fn chrome(window: &Window, visible: bool) {
    unsafe { sf_window_chrome(view_ptr(window), visible) }
}
pub fn save_window(window: &Window) {
    unsafe { sf_window_save(view_ptr(window)) }
}
pub fn available_size(window: &Window) -> crate::geometry::Extent {
    let (mut w, mut h) = (0., 0.);
    unsafe { sf_window_available(view_ptr(window), &mut w, &mut h) };
    crate::geometry::Extent {
        width: w as f32,
        height: h as f32,
    }
}
pub fn resize_window(window: &Window, size: crate::geometry::Extent) {
    unsafe { sf_window_resize(view_ptr(window), size.width as f64, size.height as f64) }
}
pub fn start_window_drag(window: &Window) {
    unsafe { sf_window_start_drag(view_ptr(window)) }
}
pub fn toggle_fullscreen(window: &Window) {
    unsafe { sf_window_toggle_fullscreen(view_ptr(window)) }
}
pub fn snapshot(
    window: &Window,
    image: &gpui::RenderImage,
    bounds: gpui::Bounds<gpui::Pixels>,
    direction: i32,
) {
    let Some(bytes) = image.as_bytes(0) else {
        return;
    };
    let size = image.size(0);
    unsafe {
        sf_window_snapshot(
            view_ptr(window),
            bytes.as_ptr(),
            size.width.0 as u32,
            size.height.0 as u32,
            f32::from(bounds.origin.x) as f64,
            f32::from(bounds.origin.y) as f64,
            f32::from(bounds.size.width) as f64,
            f32::from(bounds.size.height) as f64,
            direction,
        )
    }
}
pub fn begin_transition(window: &Window) -> bool {
    unsafe { sf_window_begin_transition(view_ptr(window)) }
}
pub fn transition(window: &Window, progress: f64) {
    unsafe { sf_window_transition(view_ptr(window), progress) }
}
pub fn finish_transition(window: &Window) {
    unsafe { sf_window_finish_transition(view_ptr(window)) }
}
pub fn complete_transition(window: &Window, epoch: u64) {
    unsafe { sf_window_complete_transition(view_ptr(window), epoch) }
}

pub fn restore_origin(window: &Window) {
    unsafe { sf_window_restore_origin(view_ptr(window)) }
}

pub fn reduce_motion() -> bool {
    unsafe { sf_reduce_motion() }
}

#[cfg(debug_assertions)]
pub fn transition_probe(window: &Window) -> (i32, [f64; 7]) {
    let mut values = [0.; 7];
    let count = unsafe { sf_window_transition_probe(view_ptr(window), values.as_mut_ptr()) };
    (count, values)
}

#[cfg(debug_assertions)]
pub fn snapshot_count(window: &Window) -> i32 {
    unsafe { sf_window_snapshot_count(view_ptr(window)) }
}
pub fn default_status() -> i32 {
    unsafe { sf_default_status() }
}
pub fn default_error() -> String {
    unsafe {
        std::ffi::CStr::from_ptr(sf_default_error())
            .to_string_lossy()
            .into_owned()
    }
}
pub fn make_default() {
    unsafe { sf_make_default() }
}
pub fn zoom_menu(mode: i32) {
    unsafe { sf_zoom_menu(mode) }
}

pub fn snapshot_video(
    window: &Window,
    frame: &CVPixelBuffer,
    bounds: gpui::Bounds<gpui::Pixels>,
    direction: i32,
) {
    unsafe {
        sf_window_snapshot_video(
            view_ptr(window),
            frame.as_concrete_TypeRef().cast(),
            f32::from(bounds.origin.x) as f64,
            f32::from(bounds.origin.y) as f64,
            f32::from(bounds.size.width) as f64,
            f32::from(bounds.size.height) as f64,
            direction,
        )
    }
}
