use crate::{
    asset_path,
    geometry::Extent,
    library::{self, MediaKind},
    loader::{self, Cache, Picture, Request},
    native::{self, Gesture, Gestures, Video},
    navigation::{Boundary, Navigation},
    swipe::SwipeMotion,
};
use core_video::pixel_buffer::CVPixelBuffer;
use gpui::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

actions!(
    petalook,
    [
        Open,
        Next,
        Previous,
        Fit,
        FitScreen,
        FitWindow,
        DefaultViewer,
        MakeDefault,
        DismissDefault,
        ActualSize,
        ZoomIn,
        ZoomOut,
        PlayPause,
        Fullscreen,
        Escape,
        ToggleInfo,
        Reveal,
        SeekBackward,
        SeekForward,
        Mute,
        Shortcuts,
        About
    ]
);

struct SwipeOrigin {
    index: usize,
    picture: Option<Picture>,
    video: Option<Video>,
    video_frame: Option<CVPixelBuffer>,
    video_size: Option<Extent>,
    video_playing: bool,
    zoom: f32,
    pan: Point<Pixels>,
    auto_fitted: bool,
    frame_basis: Option<Extent>,
}

struct SwipeTransition {
    origin: SwipeOrigin,
    motion: SwipeMotion,
    ready: bool,
    released: Option<bool>,
}

#[derive(Clone)]
struct MediaPresentation {
    picture: Option<Picture>,
    video_frame: Option<CVPixelBuffer>,
    bounds: Bounds<Pixels>,
}

pub struct Viewer {
    focus: FocusHandle,
    about_window: Option<WindowHandle<crate::ui::AboutView>>,
    files: Vec<PathBuf>,
    index: usize,
    picture: Option<Picture>,
    video: Option<Video>,
    video_frame: Option<CVPixelBuffer>,
    cache: Cache,
    requests: loader::Requests,
    generation: u64,
    open_generation: u64,
    pending_scan: Option<(u64, Vec<PathBuf>)>,
    pending_recent: Option<PathBuf>,
    zoom: f32,
    auto_zoom: i32,
    auto_fitted: bool,
    fit_window_size: Extent,
    last_viewport_size: Extent,
    was_fullscreen: bool,
    frame_basis: Option<Extent>,
    video_size: Option<Extent>,
    programmatic_until: Instant,
    transition_task: Option<Task<()>>,
    transitioning: bool,
    transition_source: Option<MediaPresentation>,
    pending_navigation: Option<Navigation>,
    transition_epoch: u64,
    transition_boundary: Option<Boundary>,
    cycle_notice: Option<(Boundary, Instant)>,
    swipe_transition: Option<SwipeTransition>,
    trackpad_active: bool,
    chrome_visible: bool,
    default_banner: bool,
    default_state: i32,
    default_task: Option<Task<()>>,
    scrollbar_drag: Option<bool>,
    detail_requested: bool,
    pan: Point<Pixels>,
    drag: Option<Point<Pixels>>,
    swipe: f32,
    swipe_fired: bool,
    last_scroll: Instant,
    error: Option<String>,
    loading: bool,
    controls: bool,
    info: bool,
    help: bool,
    muted: bool,
    video_task: Option<Task<()>>,
    _gestures: Option<Gestures>,
    first_media_frame: bool,
    startup_finished: bool,
    prefetch_generation: u64,
}

impl Viewer {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        paths: Vec<PathBuf>,
        opens: async_channel::Receiver<Vec<PathBuf>>,
        loader: loader::Loader,
    ) -> Self {
        crate::startup::mark("platform_window_ready");
        let focus = cx.focus_handle();
        window.focus(&focus);
        native::configure_window(window);
        let initial_size = window.viewport_size();
        cx.observe_window_bounds(window, |this, window, cx| {
            let viewport = window.viewport_size();
            let size = Extent {
                width: viewport.width.into(),
                height: viewport.height.into(),
            };
            let resized = this.last_viewport_size != size;
            this.last_viewport_size = size;
            let fullscreen = window.is_fullscreen();
            let changed_fullscreen = this.was_fullscreen != fullscreen;
            this.was_fullscreen = fullscreen;
            if !resized && !changed_fullscreen {
                return;
            }
            if fullscreen {
                if this.auto_fitted {
                    this.fit_image(window);
                    cx.notify();
                }
                return;
            }
            if changed_fullscreen {
                if this.auto_fitted {
                    this.apply_auto_zoom(window);
                    cx.notify();
                }
                return;
            }
            if this.transitioning || Instant::now() < this.programmatic_until {
                return;
            }
            if this.auto_fitted {
                this.apply_auto_zoom(window);
            }
            this.clamp_pan(window);
            cx.notify();
        })
        .detach();
        let loader::Loader { requests, loaded } = loader;
        let primed = loaded.try_recv().ok();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(loaded) = loaded.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        if loaded.generation != this.generation {
                            return;
                        }
                        let current = this.path() == Some(&loaded.path);
                        match loaded.result {
                            Ok(mut picture) => {
                                if this.cache.has(&loaded.path, picture.limit)
                                    && let Some(cached) = this.cache.get(&loaded.path)
                                {
                                    picture = cached;
                                }
                                if current {
                                    let first = this.loading;
                                    if first
                                        || this
                                            .picture
                                            .as_ref()
                                            .is_none_or(|p| p.limit <= picture.limit)
                                    {
                                        this.picture = Some(picture.clone());
                                        this.loading = false;
                                        this.error = None;
                                        if first {
                                            this.media_ready(window, cx);
                                        }
                                    }
                                }
                                if !this.cache.has(&loaded.path, picture.limit) {
                                    for image in this.cache.insert(loaded.path, picture) {
                                        let _ = window.drop_image(image);
                                    }
                                }
                            }
                            Err(error) if current => {
                                this.error = Some(error);
                                this.loading = false;
                                if this.swipe_transition.is_some() {
                                    this.rollback_swipe(window, cx);
                                } else {
                                    this.cancel_transition(window);
                                }
                            }
                            Err(_) => {}
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(paths) = opens.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.open_paths(paths, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let (gestures, events) = async_channel::unbounded();
        let monitor = Gestures::install(window, gestures);
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        match event {
                            Gesture::Magnify(delta) => {
                                this.set_zoom(this.zoom * (1. + delta), window, cx)
                            }
                            Gesture::SmartZoom => this.smart_zoom(window, cx),
                            Gesture::CancelSwipe => this.release_swipe(true, window, cx),
                            Gesture::TransitionFinished(epoch) => {
                                this.transition_completed(epoch, window, cx)
                            }
                            Gesture::Scroll(event) => this.scroll(&event, window, cx),
                            Gesture::Swipe(delta) if delta.abs() > 0.1 => {
                                this.navigate(if delta < 0. { 1 } else { -1 }, window, cx)
                            }
                            _ => {}
                        }
                        this.refresh_controls(window, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let mut this = Self {
            focus,
            about_window: None,
            files: vec![],
            index: 0,
            picture: None,
            video: None,
            video_frame: None,
            cache: Cache::default(),
            requests,
            generation: 0,
            open_generation: 0,
            pending_scan: None,
            pending_recent: None,
            zoom: 1.,
            auto_zoom: native::preference("autoZoom"),
            auto_fitted: true,
            fit_window_size: native::fit_size(),
            last_viewport_size: Extent {
                width: initial_size.width.into(),
                height: initial_size.height.into(),
            },
            was_fullscreen: window.is_fullscreen(),
            frame_basis: None,
            video_size: None,
            programmatic_until: Instant::now(),
            transition_task: None,
            transitioning: false,
            transition_source: None,
            pending_navigation: None,
            transition_epoch: 0,
            transition_boundary: None,
            cycle_notice: None,
            swipe_transition: None,
            trackpad_active: false,
            chrome_visible: true,
            default_banner: native::preference("defaultBannerDismissed") == 0,
            default_state: -1,
            default_task: None,
            scrollbar_drag: None,
            detail_requested: false,
            pan: point(px(0.), px(0.)),
            drag: None,
            swipe: 0.,
            swipe_fired: false,
            last_scroll: Instant::now(),
            error: None,
            loading: false,
            controls: false,
            info: false,
            help: false,
            muted: false,
            video_task: None,
            _gestures: monitor,
            first_media_frame: false,
            startup_finished: false,
            prefetch_generation: 0,
        };
        crate::startup::mark("viewer_state_ready");
        native::zoom_menu(this.auto_zoom);
        cx.spawn_in(window, async move |this, cx| {
            Timer::after(Duration::from_secs(1)).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.chrome_visible = false;
                native::chrome(window, false);
                cx.notify();
            });
        })
        .detach();
        if let Some(loaded) = primed
            && paths.first() == Some(&loaded.path)
            && let Ok(picture) = loaded.result
        {
            this.cache.insert(loaded.path, picture);
        }
        if !paths.is_empty() {
            this.open_paths(paths, window, cx);
        } else {
            let viewport = window.viewport_size();
            if viewport.width < px(420.) || viewport.height < px(500.) {
                native::resize_window(
                    window,
                    Extent {
                        width: f32::from(viewport.width).max(420.),
                        height: f32::from(viewport.height).max(500.),
                    },
                );
            }
        }
        crate::startup::mark("viewer_ready");
        #[cfg(debug_assertions)]
        if std::env::var_os("PETALOOK_SWIPE_QA").is_some() {
            qa::run(window, cx);
        }
        #[cfg(debug_assertions)]
        if std::env::var_os("PETALOOK_FIT_QA").is_some() {
            qa::run_fit(window, cx);
        }
        this
    }

    fn path(&self) -> Option<&PathBuf> {
        self.files.get(self.index)
    }

    fn open_paths(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_transition(window);
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .map(|p| std::path::absolute(&p).unwrap_or(p))
            .collect();
        self.open_generation += 1;
        let generation = self.open_generation;
        self.error = None;
        // The requested file gets the disk and decoder before its folder is enumerated.
        if paths.first().is_some_and(|p| library::kind(p).is_some()) {
            self.files = vec![paths[0].clone()];
            self.index = 0;
            self.pending_scan = Some((generation, paths));
            self.load_current(window, cx);
        } else {
            self.pending_scan = None;
            self.generation += 1;
            self.requests.send(Request {
                foreground: None,
                prefetch: vec![],
                generation: self.generation,
            });
            self.video_task = None;
            self.video = None;
            self.video_frame = None;
            self.picture = None;
            self.files.clear();
            self.index = 0;
            self.loading = true;
            self.scan_paths(paths, generation, window, cx);
        }
        self.refresh_controls(window, cx);
    }

    fn scan_paths(
        &mut self,
        paths: Vec<PathBuf>,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scan = cx
            .background_executor()
            .spawn(async move { library::collect(&paths) });
        cx.spawn_in(window, async move |this, cx| {
            let result = scan.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.open_generation {
                    return;
                }
                match result {
                    Ok((files, index)) => {
                        let same = this.path() == files.get(index);
                        this.files = files;
                        this.index = index;
                        if !same {
                            this.load_current(window, cx);
                        } else if this.video.is_none() {
                            this.request_picture(loader::PREVIEW_LIMIT);
                        }
                    }
                    Err(error) => {
                        if this.picture.is_none() && this.video_frame.is_none() {
                            this.loading = false;
                            this.error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open(&mut self, _: &Open, window: &mut Window, cx: &mut Context<Self>) {
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = selected.await {
                let _ = this.update_in(cx, |this, window, cx| this.open_paths(paths, window, cx));
            }
        })
        .detach();
    }

    fn load_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        self.detail_requested = false;
        self.video_size = None;
        self.pan = point(px(0.), px(0.));
        self.requests.send(Request {
            foreground: None,
            prefetch: vec![],
            generation: self.generation,
        });
        self.video_task = None;
        self.video = None;
        self.video_frame = None;
        if !self.transitioning {
            self.picture = None;
        }
        self.error = None;
        self.loading = true;
        let Some(path) = self.path().cloned() else {
            self.loading = false;
            return;
        };
        window.set_window_title(&format!(
            "{} — Petalook",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        self.pending_recent = Some(path.clone());
        if library::kind(&path) == Some(MediaKind::Video) {
            match Video::open(&path) {
                Ok(video) => {
                    video.mute(self.muted);
                    self.video = Some(video);
                    self.start_video(window, cx);
                }
                Err(error) => {
                    self.error = Some(error.to_string());
                    self.loading = false;
                    if self.swipe_transition.is_some() {
                        self.rollback_swipe(window, cx);
                    } else {
                        self.cancel_transition(window);
                    }
                }
            }
        } else {
            if let Some(picture) = self.cache.get(&path) {
                self.picture = Some(picture);
                self.loading = false;
                self.media_ready(window, cx);
            }
            self.request_picture(loader::PREVIEW_LIMIT);
        }
        self.refresh_controls(window, cx);
        cx.notify();
    }

    fn start_video(&mut self, window: &Window, cx: &mut Context<Self>) {
        let generation = self.generation;
        self.video_task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut interval = 16;
            loop {
                Timer::after(Duration::from_millis(interval)).await;
                let keep = this
                    .update_in(cx, |this, window, cx| {
                        if generation != this.generation {
                            return false;
                        }
                        let Some(video) = &this.video else {
                            return false;
                        };
                        interval = if video.playing() || this.loading {
                            16
                        } else {
                            100
                        };
                        if video.failed() {
                            this.error =
                                Some("This video’s codec could not be played by macOS.".into());
                            this.loading = false;
                            if this.swipe_transition.is_some() {
                                this.rollback_swipe(window, cx);
                            } else {
                                this.cancel_transition(window);
                            }
                            cx.notify();
                            return false;
                        }
                        if let Some(frame) = video.frame() {
                            let first = this.loading;
                            if first && this.swipe_transition.is_some() {
                                video.play(false);
                            }
                            this.video_size = Some(Extent {
                                width: frame.get_width() as f32 / window.scale_factor(),
                                height: frame.get_height() as f32 / window.scale_factor(),
                            });
                            this.video_frame = Some(frame);
                            this.picture = None;
                            this.loading = false;
                            if first {
                                this.media_ready(window, cx);
                            }
                            cx.notify();
                        } else if this.controls && video.playing() {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        }));
    }

    fn request_picture(&self, limit: u32) {
        let limit = if self.detail_requested {
            loader::DETAIL_LIMIT
        } else {
            limit
        };
        let Some(path) = self.path().cloned() else {
            return;
        };
        let foreground = (!self.cache.has(&path, limit)).then_some((path, limit));
        let mut prefetch = Vec::new();
        for index in [-1, 1]
            .map(|direction| {
                Navigation::step(self.index, self.files.len(), direction).map(|target| target.index)
            })
            .into_iter()
            .flatten()
        {
            if let Some(path) = self.files.get(index)
                && library::kind(path) == Some(MediaKind::Image)
                && !self.loading
                && self.prefetch_generation == self.generation
                && !self.cache.has(path, loader::PREVIEW_LIMIT)
            {
                prefetch.push((path.clone(), loader::PREVIEW_LIMIT));
            }
        }
        self.requests.send(Request {
            foreground,
            prefetch,
            generation: self.generation,
        });
    }

    fn navigate(&mut self, direction: i32, window: &mut Window, cx: &mut Context<Self>) {
        let source = self
            .pending_navigation
            .map_or(self.index, |pending| pending.index);
        let Some(target) = Navigation::step(source, self.files.len(), direction) else {
            return;
        };
        if self.transitioning && !self.loading {
            self.pending_navigation = Some(target);
            return;
        }
        self.navigate_to(target, window, cx);
    }
    fn navigate_to(&mut self, target: Navigation, window: &mut Window, cx: &mut Context<Self>) {
        if target.index != self.index {
            if !self.transitioning {
                self.cancel_transition(window);
                let source = self.live_presentation(window);
                if let Some(picture) = &self.picture {
                    native::snapshot(
                        window,
                        &picture.image,
                        self.media_bounds(window),
                        target.direction,
                    );
                    self.transitioning = true;
                } else if let Some(frame) = &self.video_frame {
                    native::snapshot_video(
                        window,
                        frame,
                        self.media_bounds(window),
                        target.direction,
                    );
                    self.transitioning = true;
                }
                if self.transitioning {
                    self.transition_source = Some(source);
                }
            }
            self.transition_epoch += 1;
            self.transition_task = None;
            self.transition_boundary = target.boundary;
            self.cycle_notice = None;
            self.index = target.index;
            self.load_current(window, cx);
        }
        self.refresh_controls(window, cx);
    }

    fn next(&mut self, _: &Next, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(1, window, cx);
    }
    fn previous(&mut self, _: &Previous, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(-1, window, cx);
    }
    fn media_extent(&self, window: &Window) -> Option<Extent> {
        self.picture
            .as_ref()
            .map(|picture| Extent {
                width: picture.original.0 as f32 / window.scale_factor(),
                height: picture.original.1 as f32 / window.scale_factor(),
            })
            .or(self.video_size)
    }
    fn media_bounds(&self, window: &Window) -> Bounds<Pixels> {
        let viewport = window.viewport_size();
        let extent = self
            .media_extent(window)
            .unwrap_or(Extent {
                width: 1.,
                height: 1.,
            })
            .scaled(self.zoom);
        let dimensions = size(px(extent.width), px(extent.height));
        Bounds::new(
            point(
                (viewport.width - dimensions.width) / 2. + self.pan.x,
                (viewport.height - dimensions.height) / 2. + self.pan.y,
            ),
            dimensions,
        )
    }

    fn live_presentation(&self, window: &Window) -> MediaPresentation {
        // WindowServer opacity and Metal presentation can reach the display in
        // different frames. Never submit the target to the live window mid-swipe.
        if let Some(source) = &self.transition_source {
            return source.clone();
        }
        MediaPresentation {
            picture: self.picture.clone(),
            video_frame: self.video_frame.clone(),
            bounds: self.media_bounds(window),
        }
    }
    fn overflowing(&self, window: &Window) -> bool {
        let bounds = self.media_bounds(window);
        bounds.size.width > window.viewport_size().width + px(1.)
            || bounds.size.height > window.viewport_size().height + px(1.)
    }
    fn fit_image(&mut self, window: &Window) {
        if let Some(extent) = self.media_extent(window) {
            let viewport = window.viewport_size();
            self.zoom = extent.fit_scale(Extent {
                width: viewport.width.into(),
                height: viewport.height.into(),
            });
            self.pan = point(px(0.), px(0.));
        }
    }
    fn apply_auto_zoom(&mut self, window: &Window) {
        self.auto_fitted = true;
        self.programmatic_until = Instant::now() + Duration::from_millis(350);
        if let Some(extent) = self.media_extent(window) {
            if window.is_fullscreen() {
                self.fit_image(window);
            } else {
                let screen = native::available_size(window);
                let maximum = if self.auto_zoom == 0 {
                    Extent {
                        width: self.fit_window_size.width.min(screen.width),
                        height: self.fit_window_size.height.min(screen.height),
                    }
                } else {
                    screen
                };
                self.zoom = extent.fit_scale(maximum);
                if self.auto_zoom == 1 {
                    self.zoom = self.zoom.min(1.);
                }
                native::resize_window(window, extent.scaled(self.zoom));
            }
            self.frame_basis = Some(extent);
        }
        self.pan = point(px(0.), px(0.));
    }
    fn media_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_auto_zoom(window);
        if !self.transitioning {
            if let Some(boundary) = self.transition_boundary.take() {
                self.cycle_notice = Some((boundary, Instant::now()));
            }
            return;
        }
        let epoch = self.transition_epoch;
        self.transition_task = Some(cx.spawn_in(window, async move |this, cx| {
            // AppKit must deliver the final resize before capturing incoming media bounds.
            Timer::after(Duration::from_millis(16)).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this.transitioning || this.transition_epoch != epoch {
                    return;
                }
                let bounds = this.media_bounds(window);
                // Direction zero fills the incoming snapshot of the existing transition.
                if let Some(picture) = &this.picture {
                    native::snapshot(window, &picture.image, bounds, 0);
                } else if let Some(frame) = &this.video_frame {
                    native::snapshot_video(window, frame, bounds, 0);
                }
                let animated = native::begin_transition(window);
                if let Some(swipe) = &mut this.swipe_transition {
                    swipe.ready = true;
                    let progress = swipe.motion.progress();
                    native::transition(window, progress);
                    if let Some(commit) = swipe.released {
                        cx.defer_in(window, move |this, window, cx| {
                            if !this.transitioning || this.transition_epoch != epoch {
                                return;
                            }
                            this.animate_transition(
                                progress,
                                if commit { 1. } else { 0. },
                                window,
                                cx,
                            );
                        });
                    }
                } else {
                    native::transition(window, 0.);
                    cx.defer_in(window, move |this, window, cx| {
                        if !this.transitioning || this.transition_epoch != epoch {
                            return;
                        }
                        this.animate_transition(if animated { 0. } else { 1. }, 1., window, cx);
                    });
                }
            });
        }));
    }

    fn animate_transition(&mut self, from: f64, to: f64, window: &Window, cx: &mut Context<Self>) {
        let epoch = self.transition_epoch;
        let base_duration = if cfg!(debug_assertions) {
            std::env::var("PETALOOK_TRANSITION_MS")
                .ok()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite())
                .map(|value| value.clamp(100., 10000.) / 1000.)
                .unwrap_or(0.28)
        } else {
            0.28
        };
        let interactive = self.swipe_transition.is_some();
        let duration = base_duration * (to - from).abs().sqrt().max(0.35);
        self.transition_task = Some(cx.spawn_in(window, async move |this, cx| {
            let start = Instant::now();
            loop {
                let time = (start.elapsed().as_secs_f64() / duration).min(1.);
                let eased = if interactive {
                    1. - (1. - time).powi(3)
                } else {
                    time * time * (3. - 2. * time)
                };
                let progress = from + (to - from) * eased;
                if this
                    .update_in(cx, |this, window, cx| {
                        if !this.transitioning || this.transition_epoch != epoch {
                            return;
                        }
                        native::transition(window, progress);
                        if time >= 1. {
                            cx.defer_in(window, move |this, window, cx| {
                                if !this.transitioning || this.transition_epoch != epoch {
                                    return;
                                }
                                if to == 0. {
                                    this.rollback_swipe(window, cx);
                                } else {
                                    this.complete_transition(window, cx);
                                }
                            });
                        }
                    })
                    .is_err()
                    || time >= 1.
                {
                    break;
                }
                Timer::after(Duration::from_millis(8)).await;
            }
        }));
    }

    fn complete_transition(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.transition_source = None;
        if self.swipe_transition.take().is_some()
            && let Some(video) = &self.video
        {
            video.play(true);
        }
        native::complete_transition(window, self.transition_epoch);
        cx.notify();
    }

    fn transition_completed(&mut self, epoch: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.transitioning || self.transition_epoch != epoch {
            return;
        }
        self.transitioning = false;
        if let Some(boundary) = self.transition_boundary.take() {
            self.cycle_notice = Some((boundary, Instant::now()));
        }
        self.programmatic_until = Instant::now() + Duration::from_millis(50);
        self.refresh_controls(window, cx);
        if std::env::var_os("PETALOOK_TRACE").is_some() {
            eprintln!("transition finished: index={}", self.index);
        }
        let epoch = self.transition_epoch;
        if let Some(target) = self.pending_navigation.take() {
            cx.defer_in(window, move |this, window, cx| {
                if !this.transitioning && this.transition_epoch == epoch {
                    this.navigate_to(target, window, cx);
                }
            });
        }
    }

    fn begin_swipe(&mut self, direction: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = Navigation::step(self.index, self.files.len(), direction) else {
            return;
        };
        if self.transitioning || self.loading {
            return;
        }
        let source = self.live_presentation(window);
        if let Some(picture) = &self.picture {
            native::snapshot(window, &picture.image, self.media_bounds(window), direction);
        } else if let Some(frame) = &self.video_frame {
            native::snapshot_video(window, frame, self.media_bounds(window), direction);
        } else {
            return;
        }
        let video_playing = self.video.as_ref().is_some_and(|video| video.playing());
        if let Some(video) = &self.video {
            video.play(false);
        }
        self.swipe_transition = Some(SwipeTransition {
            origin: SwipeOrigin {
                index: self.index,
                picture: self.picture.clone(),
                video: self.video.take(),
                video_frame: self.video_frame.clone(),
                video_size: self.video_size,
                video_playing,
                zoom: self.zoom,
                pan: self.pan,
                auto_fitted: self.auto_fitted,
                frame_basis: self.frame_basis,
            },
            motion: SwipeMotion::new(direction, f32::from(window.viewport_size().width) as f64),
            ready: false,
            released: None,
        });
        self.transitioning = true;
        self.transition_source = Some(source);
        self.transition_epoch += 1;
        self.transition_boundary = target.boundary;
        self.cycle_notice = None;
        self.index = target.index;
        self.load_current(window, cx);
    }

    fn release_swipe(&mut self, cancelled: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.trackpad_active = false;
        if let Some(swipe) = &mut self.swipe_transition {
            if swipe.released.is_some() {
                return;
            }
            let commit = !cancelled
                && swipe
                    .motion
                    .commits(self.last_scroll.elapsed().as_secs_f64());
            swipe.released = Some(commit);
            let progress = swipe.motion.progress();
            if swipe.ready {
                self.animate_transition(progress, if commit { 1. } else { 0. }, window, cx);
            } else if !commit {
                self.rollback_swipe(window, cx);
            }
        }
    }

    fn rollback_swipe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(swipe) = self.swipe_transition.take() else {
            return;
        };
        self.transition_epoch += 1;
        self.transition_task = None;
        self.transition_boundary = None;
        self.generation += 1;
        self.requests.send(Request {
            foreground: None,
            prefetch: vec![],
            generation: self.generation,
        });
        self.video_task = None;
        self.index = swipe.origin.index;
        self.picture = swipe.origin.picture;
        self.video = swipe.origin.video;
        self.video_frame = swipe.origin.video_frame;
        self.video_size = swipe.origin.video_size;
        self.zoom = swipe.origin.zoom;
        self.pan = swipe.origin.pan;
        self.auto_fitted = swipe.origin.auto_fitted;
        self.frame_basis = swipe.origin.frame_basis;
        self.loading = false;
        self.error = None;
        self.pending_navigation = None;
        self.trackpad_active = false;
        if let Some(video) = &self.video {
            video.play(swipe.origin.video_playing);
            self.start_video(window, cx);
        }
        if let Some(path) = self.path() {
            window.set_window_title(&format!(
                "{} — Petalook",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        native::restore_origin(window);
        cx.notify();
        self.complete_transition(window, cx);
    }

    fn cancel_transition(&mut self, window: &Window) {
        self.transition_source = None;
        self.transition_epoch += 1;
        self.transition_task = None;
        self.pending_navigation = None;
        self.transition_boundary = None;
        self.cycle_notice = None;
        if self.swipe_transition.take().is_some()
            && let Some(video) = &self.video
        {
            video.play(true);
        }
        self.trackpad_active = false;
        native::finish_transition(window);
        self.transitioning = false;
    }
    fn fit(&mut self, _: &Fit, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_transition(window);
        self.apply_auto_zoom(window);
        self.refresh_controls(window, cx);
    }
    fn fit_screen(&mut self, _: &FitScreen, window: &mut Window, cx: &mut Context<Self>) {
        self.auto_zoom = 1;
        native::set_preference("autoZoom", 1);
        native::zoom_menu(1);
        self.fit(&Fit, window, cx);
    }
    fn fit_window(&mut self, _: &FitWindow, window: &mut Window, cx: &mut Context<Self>) {
        self.auto_zoom = 0;
        native::set_preference("autoZoom", 0);
        native::zoom_menu(0);
        self.fit(&Fit, window, cx);
    }
    fn set_zoom(&mut self, zoom: f32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(extent) = self.media_extent(window) else {
            return;
        };
        if self.loading || !zoom.is_finite() {
            return;
        }
        self.cancel_transition(window);
        self.auto_fitted = false;
        self.zoom = zoom.clamp(0.02, 32.);
        self.programmatic_until = Instant::now() + Duration::from_millis(250);
        if !window.is_fullscreen() {
            let frame = self
                .frame_basis
                .unwrap_or(extent)
                .frame_at(self.zoom, native::available_size(window));
            native::resize_window(window, frame);
            self.fit_window_size = frame;
            native::save_fit_size(frame);
            native::save_window(window);
        }
        self.clamp_pan(window);
        if !self.detail_requested
            && self.picture.as_ref().is_some_and(|p| {
                p.limit < loader::DETAIL_LIMIT
                    && p.original.0.max(p.original.1) > loader::PREVIEW_LIMIT
                    && (extent.width.max(extent.height) * self.zoom * window.scale_factor())
                        > loader::PREVIEW_LIMIT as f32
            })
        {
            self.detail_requested = true;
            self.request_picture(loader::DETAIL_LIMIT);
        }
        self.refresh_controls(window, cx);
    }
    fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom * 1.2, window, cx);
    }
    fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom / 1.2, window, cx);
    }
    fn actual_size(&mut self, _: &ActualSize, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(1., window, cx);
    }
    fn smart_zoom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.auto_fitted {
            self.set_zoom(self.zoom * 2., window, cx);
        } else {
            self.fit(&Fit, window, cx);
        }
    }
    fn clamp_pan(&mut self, window: &Window) {
        if let Some(extent) = self.media_extent(window) {
            let extent = extent.scaled(self.zoom);
            let viewport = window.viewport_size();
            let x = ((extent.width - f32::from(viewport.width)) / 2.).max(0.);
            let y = ((extent.height - f32::from(viewport.height)) / 2.).max(0.);
            self.pan.x = px(f32::from(self.pan.x).clamp(-x, x));
            self.pan.y = px(f32::from(self.pan.y).clamp(-y, y));
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(24.));
        let (x, y) = (f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.platform || event.modifiers.control {
            self.set_zoom(self.zoom * (y * 0.008).exp(), window, cx);
        } else if self.overflowing(window) && self.swipe_transition.is_none() {
            self.pan.x += delta.x;
            self.pan.y += delta.y;
            self.clamp_pan(window);
            cx.notify();
        } else if matches!(event.delta, ScrollDelta::Pixels(_)) {
            if matches!(event.touch_phase, TouchPhase::Started) {
                if self.transitioning {
                    return;
                }
                self.trackpad_active = true;
                self.swipe = 0.;
                self.last_scroll = Instant::now();
            }
            if !self.trackpad_active {
                return;
            } // Ignore momentum after fingers lift.
            if matches!(event.touch_phase, TouchPhase::Ended) {
                if self.swipe_transition.is_some() {
                    self.release_swipe(false, window, cx);
                } else {
                    self.trackpad_active = false;
                    if self.swipe.abs() >= 70.
                        && (window.is_fullscreen() || native::reduce_motion())
                    {
                        self.navigate(if self.swipe < 0. { 1 } else { -1 }, window, cx);
                    }
                }
                return;
            }
            let elapsed = self.last_scroll.elapsed().as_secs_f64();
            if x != 0. {
                self.last_scroll = Instant::now();
            }
            if self.swipe_transition.is_none() && x.abs() > y.abs() {
                self.swipe += x;
                if self.swipe.abs() >= 8. && !window.is_fullscreen() && !native::reduce_motion() {
                    self.begin_swipe(if self.swipe < 0. { 1 } else { -1 }, window, cx);
                    if let Some(swipe) = &mut self.swipe_transition {
                        swipe.motion.update(self.swipe as f64, elapsed);
                    }
                }
            } else if let Some(swipe) = &mut self.swipe_transition {
                swipe.motion.update(x as f64, elapsed);
                if swipe.ready {
                    native::transition(window, swipe.motion.progress());
                }
            }
        } else {
            if self.last_scroll.elapsed() > Duration::from_millis(220) {
                self.swipe = 0.;
                self.swipe_fired = false;
            }
            self.last_scroll = Instant::now();
            if x.abs() > y.abs() && !self.swipe_fired {
                self.swipe += x;
                if self.swipe.abs() >= 70. {
                    self.swipe_fired = true;
                    self.navigate(if self.swipe < 0. { 1 } else { -1 }, window, cx);
                }
            }
        }
    }

    fn refresh_controls(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.controls &= self.near_controls(window.mouse_position(), window);
        cx.notify();
    }

    fn near_controls(&self, position: Point<Pixels>, window: &Window) -> bool {
        let viewport = window.viewport_size();
        let height = 88.;
        position.x >= px(0.)
            && position.x <= viewport.width
            && position.y >= viewport.height - px(height)
            && position.y <= viewport.height
    }

    fn play_pause(&mut self, _: &PlayPause, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(video) = &self.video {
            video.play(!video.playing());
        }
        self.refresh_controls(window, cx);
    }
    fn seek_backward(&mut self, _: &SeekBackward, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(v) = &self.video {
            v.seek(v.time() - 5.);
        }
        self.refresh_controls(window, cx);
    }
    fn seek_forward(&mut self, _: &SeekForward, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(v) = &self.video {
            v.seek(v.time() + 5.);
        }
        self.refresh_controls(window, cx);
    }
    fn mute(&mut self, _: &Mute, window: &mut Window, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        if let Some(v) = &self.video {
            v.mute(self.muted);
        }
        self.refresh_controls(window, cx);
    }
    fn fullscreen(&mut self, _: &Fullscreen, window: &mut Window, _: &mut Context<Self>) {
        self.cancel_transition(window);
        native::toggle_fullscreen(window);
    }
    fn toggle_info(&mut self, _: &ToggleInfo, window: &mut Window, cx: &mut Context<Self>) {
        self.info = !self.info;
        if std::env::var_os("PETALOOK_TRACE").is_some() {
            eprintln!(
                "info={}, transitioning={}, loading={}",
                self.info, self.transitioning, self.loading
            );
        }
        self.refresh_controls(window, cx);
    }
    fn shortcuts(&mut self, _: &Shortcuts, window: &mut Window, cx: &mut Context<Self>) {
        self.help = !self.help;
        self.refresh_controls(window, cx);
    }
    fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.swipe_transition.is_some() {
            self.release_swipe(true, window, cx);
        } else if self.help || self.info {
            self.help = false;
            self.info = false;
        } else if window.is_fullscreen() {
            native::toggle_fullscreen(window);
        } else {
            self.fit(&Fit, window, cx);
        }
        cx.notify();
    }
    fn reveal(&mut self, _: &Reveal, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.path() {
            cx.reveal_path(path);
        }
    }
    fn about(&mut self, _: &About, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.about_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        match crate::ui::AboutView::open(cx) {
            Ok(handle) => self.about_window = Some(handle),
            Err(error) => eprintln!("Could not open About Petalook: {error}"),
        }
    }

    fn default_viewer(&mut self, _: &DefaultViewer, _: &mut Window, cx: &mut Context<Self>) {
        self.default_banner = true;
        self.default_state = native::default_status();
        native::set_preference("defaultBannerDismissed", 0);
        cx.notify();
    }
    fn dismiss_default(&mut self, _: &DismissDefault, _: &mut Window, cx: &mut Context<Self>) {
        self.default_banner = false;
        native::set_preference("defaultBannerDismissed", 1);
        cx.notify();
    }
    fn make_default(&mut self, _: &MakeDefault, window: &mut Window, cx: &mut Context<Self>) {
        native::make_default();
        self.default_state = native::default_status();
        self.default_task = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(150)).await;
                let busy = this
                    .update_in(cx, |this, _, cx| {
                        this.default_state = native::default_status();
                        cx.notify();
                        this.default_state == 2
                    })
                    .unwrap_or(false);
                if !busy {
                    break;
                }
            }
        }));
        cx.notify();
    }
    fn default_prompt(&self, window: &Window) -> impl IntoElement {
        let empty = self.files.is_empty();
        let narrow = window.viewport_size().width < px(350.);
        let tiny = !empty
            && (window.viewport_size().height < px(180.)
                || window.viewport_size().width < px(240.));
        let (title, detail) = match self.default_state {
            1 => (
                "You're all set",
                "Images and videos open in Petalook.".to_owned(),
            ),
            2 => (
                "Finish in macOS",
                "Confirm the file types you want to open here.".to_owned(),
            ),
            3 => ("Some file types weren't changed", native::default_error()),
            -1 => ("Default Viewer", "Checking your preferences…".to_owned()),
            _ => (
                "Open with Petalook",
                "Open images and videos directly in Petalook.".to_owned(),
            ),
        };
        let actionable = self.default_state == 0 || self.default_state == 3;
        let mut body = div().flex().flex_col().gap(px(14.)).flex_1().min_w_0();
        if !tiny {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(rgb(crate::ui::MUTED))
                            .child(detail),
                    ),
            );
        }
        if actionable {
            body = body.child(
                div().flex().child(
                    control(
                        "make-default",
                        if tiny {
                            "Default…"
                        } else {
                            "Make Default…"
                        },
                        "Use Petalook to open supported images and videos",
                        MakeDefault,
                    )
                    .h(px(32.)),
                ),
            );
        } else if tiny {
            body = body.child(div().text_size(px(12.)).child(title));
        }
        let mut card = div()
            .relative()
            .w_full()
            .max_w(px(440.))
            .flex()
            .items_start()
            .gap(px(12.))
            .p(px(if narrow { 12. } else { 16. }))
            .rounded(px(20.))
            .text_color(rgb(crate::ui::TEXT))
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgb(0x2d2c32), 0.),
                linear_color_stop(rgb(0x27262c), 1.),
            ))
            .border_1()
            .border_color(rgba(0xf3d6ed24))
            .shadow(vec![BoxShadow {
                color: rgba(0x00000045).into(),
                offset: point(px(0.), px(8.)),
                blur_radius: px(24.),
                spread_radius: px(-3.),
            }])
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if !narrow && !tiny {
            card = card.child(
                div()
                    .size(px(32.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(rgba(0xb7a9bf15))
                    .border_1()
                    .border_color(rgba(0xb7a9bf28))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(if self.default_state == 1 {
                                "check"
                            } else {
                                "open"
                            })
                            .size(px(16.))
                            .text_color(rgb(crate::ui::ACCENT)),
                    ),
            );
        }
        card = card.child(body).child(
            icon_control(
                "dismiss-default",
                "close",
                "Dismiss this prompt",
                DismissDefault,
            )
            .size(px(28.)),
        );
        let mut placement = div().flex().justify_center();
        if empty {
            placement = placement.w_full();
        } else {
            placement = placement
                .absolute()
                .top(px(if tiny { 8. } else { 48. }))
                .left(px(12.))
                .right(px(12.));
        }
        placement.child(card)
    }

    fn scrollbars(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if self.loading {
            return vec![];
        }
        let viewport = window.viewport_size();
        let media = self.media_bounds(window).size;
        let mut bars = vec![];
        for horizontal in [true, false] {
            let (visible, content, pan) = if horizontal {
                (
                    f32::from(viewport.width),
                    f32::from(media.width),
                    f32::from(self.pan.x),
                )
            } else {
                (
                    f32::from(viewport.height),
                    f32::from(media.height),
                    f32::from(self.pan.y),
                )
            };
            if content <= visible + 1. {
                continue;
            }
            let track = visible - 12.;
            let thumb = (track * visible / content).max(28.).min(track);
            let offset = ((content - visible) / 2. - pan) / (content - visible) * (track - thumb);
            let mut rail = div()
                .id(if horizontal {
                    "horizontal-scroll"
                } else {
                    "vertical-scroll"
                })
                .absolute()
                .rounded_full()
                .bg(rgba(0x00000025))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.drag = Some(event.position);
                        this.scrollbar_drag = Some(horizontal);
                        let coordinate = f32::from(if horizontal {
                            event.position.x
                        } else {
                            event.position.y
                        }) - 6.;
                        let fraction = ((coordinate - thumb / 2.) / (track - thumb)).clamp(0., 1.);
                        let pan = (content - visible) * (0.5 - fraction);
                        if horizontal {
                            this.pan.x = px(pan);
                        } else {
                            this.pan.y = px(pan);
                        }
                        this.clamp_pan(window);
                        cx.notify();
                    }),
                );
            let mut handle = div().absolute().rounded_full().bg(rgba(0xffffff90));
            if horizontal {
                rail = rail.left(px(6.)).bottom(px(3.)).w(px(track)).h(px(8.));
                handle = handle.left(px(offset)).top(px(1.)).w(px(thumb)).h(px(6.));
            } else {
                rail = rail.right(px(3.)).top(px(6.)).w(px(8.)).h(px(track));
                handle = handle.top(px(offset)).left(px(1.)).h(px(thumb)).w(px(6.));
            }
            bars.push(rail.child(handle).into_any_element());
        }
        bars
    }
    fn toolbar(&self, window: &Window) -> impl IntoElement {
        let filename = self
            .path()
            .and_then(|p| p.file_name())
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        div()
            .w_full()
            .max_w((window.viewport_size().width - px(24.)).min(px(720.)))
            .flex()
            .items_center()
            .gap(px(10.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(icon_control(
                "open",
                "open",
                "Open a file or folder · ⌘O",
                Open,
            ))
            .child(
                crate::ui::button("filename", false, Reveal)
                    .flex_1()
                    .min_w_0()
                    .h(px(32.))
                    .px(px(8.))
                    .border_0()
                    .bg(gpui::transparent_black())
                    .shadow(vec![])
                    .hover(|s| s.bg(rgba(0xffffff12)))
                    .active(|s| s.bg(rgba(0xffffff1c)))
                    .tooltip(|_, cx| cx.new(|_| Tip("Show in Finder · ⇧⌘R")).into())
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_center()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(filename),
                    ),
            )
            .child(icon_control(
                "info",
                "info",
                "File information · ⌘I",
                ToggleInfo,
            ))
    }

    fn welcome(&self, window: &Window) -> impl IntoElement {
        let compact = window.viewport_size().height < px(520.);
        let mut content = div()
            .relative()
            .w_full()
            .max_w(px(440.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                img(asset_path("app-icon.png"))
                    .size(px(if compact { 96. } else { 120. }))
                    .mb(px(16.)),
            )
            .child(
                div()
                    .text_size(px(28.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(crate::ui::TEXT))
                    .child("Petalook"),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(13.))
                    .text_center()
                    .text_color(rgb(crate::ui::MUTED))
                    .child("A quiet place for your images and videos."),
            )
            .child(
                crate::ui::button("open-welcome", true, Open)
                    .mt(px(28.))
                    .h(px(42.))
                    .px(px(22.))
                    .child(
                        svg()
                            .path("open")
                            .size(px(16.))
                            .text_color(rgb(crate::ui::BUTTON_INK)),
                    )
                    .child("Open File or Folder")
                    .child(div().ml(px(12.)).text_size(px(11.)).child("⌘O")),
            )
            .child(
                div()
                    .mt(px(14.))
                    .text_size(px(12.))
                    .text_color(rgb(crate::ui::MUTED))
                    .child("Drop images or videos here"),
            );
        if self.default_banner {
            content = content.child(
                div()
                    .mt(px(30.))
                    .w_full()
                    .child(self.default_prompt(window)),
            );
        }
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(crate::ui::backdrop(window.viewport_size()))
            .child(
                div()
                    .id("welcome-content")
                    .relative()
                    .size_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .px(px(28.))
                    .py(px(40.))
                    .child(content),
            )
    }

    fn cycle_indicator(&mut self, window: &Window) -> Option<AnyElement> {
        let (boundary, started) = self.cycle_notice?;
        let elapsed = started.elapsed().as_secs_f32();
        if elapsed >= 1.1 {
            self.cycle_notice = None;
            return None;
        }
        window.request_animation_frame();
        let opacity = (elapsed / 0.10).min(1.) * ((1.1 - elapsed) / 0.3).clamp(0., 1.);
        let turn = if native::reduce_motion() {
            0.
        } else {
            (elapsed / 0.55).min(1.)
        };
        let turn = 1. - (1. - turn).powi(3);
        let (title, detail, direction) = match boundary {
            Boundary::End => ("End reached", "Continuing from first", 1.),
            Boundary::Beginning => ("Beginning reached", "Continuing from last", -1.),
        };
        let compact =
            window.viewport_size().width < px(240.) || window.viewport_size().height < px(100.);
        let mut badge = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .py(px(11.))
            .rounded_xl()
            .bg(rgba(0x15161bba))
            .border_1()
            .border_color(rgba(0xffffff18))
            .opacity(opacity)
            .child(
                svg()
                    .path("cycle")
                    .size(px(22.))
                    .text_color(rgb(0xf1f1f3))
                    .with_transformation(Transformation::rotate(percentage(
                        (direction * turn).rem_euclid(1.),
                    ))),
            );
        if !compact {
            badge = badge.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(0xc3c5cd))
                            .child(detail),
                    ),
            );
        }
        Some(
            div()
                .absolute()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(badge)
                .into_any_element(),
        )
    }

    fn help_panel(&self) -> impl IntoElement {
        let shortcuts = [
            ("Open file or folder", "⌘O"),
            ("Previous / next file", "← / →"),
            ("Zoom", "Pinch / ⌘+ / ⌘−"),
            ("Fit / actual size", "⌘0 / ⌘1"),
            ("Move window · snap to center", "Drag anywhere on image"),
            ("Pan a zoomed image", "Two-finger scroll / scrollbars"),
            ("Play / pause video", "Space"),
            ("Seek 5 seconds", "⌥← / ⌥→"),
            ("Mute / unmute", "M"),
            ("Full screen", "⌃⌘F"),
            ("Show in Finder", "⇧⌘R"),
            ("Close this panel", "Esc"),
        ];
        div()
            .w(px(390.))
            .p_6()
            .rounded_xl()
            .bg(rgb(0x27282d))
            .border_1()
            .border_color(rgb(0x42434a))
            .shadow_lg()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .mb_4()
                    .child(div().text_size(px(18.)).child("Make yourself at home"))
                    .child(control("close-help", "×", "Close shortcuts", Shortcuts)),
            )
            .children(shortcuts.into_iter().map(|(label, key)| {
                div()
                    .flex()
                    .justify_between()
                    .py_2()
                    .text_size(px(12.))
                    .child(label)
                    .child(div().text_color(rgb(0xb7bac4)).child(key))
            }))
    }
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_media_frame && (self.picture.is_some() || self.video_frame.is_some()) {
            self.first_media_frame = true;
            crate::startup::mark("first_media_render");
            window.on_next_frame(|_, _| crate::startup::mark("first_media_frame"));
        }
        if !self.loading
            && (self.picture.is_some() || self.video_frame.is_some())
            && let Some(path) = self.pending_recent.take()
        {
            window.on_next_frame(move |_, cx| cx.add_recent_document(&path));
        }
        if !self.loading
            && let Some((generation, paths)) = self.pending_scan.take()
        {
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.open_generation == generation {
                        this.scan_paths(paths, generation, window, cx);
                    }
                });
            });
        }
        if !self.loading && !self.startup_finished {
            self.startup_finished = true;
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |this, cx| {
                    native::set_icon(&asset_path("app-icon.png"));
                    native::save_fit_size(this.fit_window_size);
                    if this.default_banner && this.default_state == -1 {
                        this.default_state = native::default_status();
                        cx.notify();
                    }
                    crate::startup::mark("deferred_setup_complete");
                    crate::updater::start();
                });
            });
        }
        if !self.loading && self.picture.is_some() && self.prefetch_generation != self.generation {
            let generation = self.generation;
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |this, _| {
                    if !this.loading
                        && this.picture.is_some()
                        && this.generation == generation
                        && this.prefetch_generation != generation
                    {
                        this.prefetch_generation = generation;
                        this.request_picture(loader::PREVIEW_LIMIT);
                    }
                });
            });
        }
        let empty = self.files.is_empty();
        let background = if empty { crate::ui::CANVAS } else { 0x17181b };
        self.clamp_pan(window);
        let viewport = window.viewport_size();
        let mut root = div()
            .id("viewer")
            .key_context("Viewer")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .font_family(".AppleSystemUIFont")
            .text_size(px(13.))
            .text_color(rgb(0xf1f1f3))
            .bg(rgb(background))
            .on_key_down(crate::ui::tab_navigation)
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::fit))
            .on_action(cx.listener(Self::fit_screen))
            .on_action(cx.listener(Self::fit_window))
            .on_action(cx.listener(Self::default_viewer))
            .on_action(cx.listener(Self::make_default))
            .on_action(cx.listener(Self::dismiss_default))
            .on_action(cx.listener(Self::actual_size))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::play_pause))
            .on_action(cx.listener(Self::seek_backward))
            .on_action(cx.listener(Self::seek_forward))
            .on_action(cx.listener(Self::mute))
            .on_action(cx.listener(Self::fullscreen))
            .on_action(cx.listener(Self::toggle_info))
            .on_action(cx.listener(Self::shortcuts))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::reveal))
            .on_action(cx.listener(Self::about))
            .on_action(cx.listener(|this, _: &crate::Close, window, _| {
                this.cancel_transition(window);
                native::save_window(window);
                window.remove_window();
            }))
            .on_action(|_: &crate::Minimize, window, _| window.minimize_window())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.open_paths(paths.paths().to_vec(), window, cx);
            }))
            .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                if !hovered {
                    this.chrome_visible = false;
                    native::chrome(window, false);
                    this.controls = false;
                    cx.notify();
                }
            }))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.controls = this.near_controls(event.position, window);
                let near_top = event.position.y < px(44.);
                if this.chrome_visible != near_top {
                    this.chrome_visible = near_top;
                    native::chrome(window, near_top);
                }
                if let Some(last) = this.drag {
                    let extent = this.media_bounds(window).size;
                    let viewport = window.viewport_size();
                    match this.scrollbar_drag {
                        Some(true) => {
                            this.pan.x -= (event.position.x - last.x)
                                * (f32::from(extent.width) / f32::from(viewport.width))
                        }
                        Some(false) => {
                            this.pan.y -= (event.position.y - last.y)
                                * (f32::from(extent.height) / f32::from(viewport.height))
                        }
                        None => {
                            this.pan.x += event.position.x - last.x;
                            this.pan.y += event.position.y - last.y;
                        }
                    }
                    this.drag = Some(event.position);
                    this.clamp_pan(window);
                }
                this.refresh_controls(window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus);
                    if event.click_count == 2 {
                        this.smart_zoom(window, cx);
                    } else {
                        native::start_window_drag(window);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.drag = None;
                    this.scrollbar_drag = None;
                }),
            );
        if empty {
            root = root.child(self.welcome(window));
        }
        let presentation = self.live_presentation(window);
        let bounds = presentation.bounds;
        if let Some(picture) = &presentation.picture {
            root = root.child(
                img(picture.image.clone())
                    .absolute()
                    .left(bounds.origin.x)
                    .top(bounds.origin.y)
                    .w(bounds.size.width)
                    .h(bounds.size.height)
                    .object_fit(ObjectFit::Contain),
            );
        }
        if let Some(frame) = &presentation.video_frame {
            root = root.child(
                surface(frame.clone())
                    .absolute()
                    .left(bounds.origin.x)
                    .top(bounds.origin.y)
                    .w(bounds.size.width)
                    .h(bounds.size.height),
            );
        }
        root = root.children(self.scrollbars(window, cx));
        if self.loading && self.picture.is_none() && self.video_frame.is_none() {
            root = root.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(0xa4a7b0))
                    .child("Opening…"),
            );
        }
        if let Some(error) = &self.error {
            root = root.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .bg(rgba(0x17181bef))
                    .child(div().text_size(px(20.)).child("Couldn’t open this file"))
                    .child(
                        div()
                            .max_w(px(400.))
                            .text_center()
                            .text_color(rgb(0xb8bbc4))
                            .child(error.clone()),
                    )
                    .child(control(
                        "retry-open",
                        "Open Another File",
                        "Open · ⌘O",
                        Open,
                    )),
            );
        }
        if self.controls
            && !empty
            && !self.transitioning
            && (!self.default_banner || viewport.height >= px(280.))
        {
            let bottom = div()
                .absolute()
                .bottom_0()
                .left_0()
                .w_full()
                .px(px(12.))
                .pt(px(32.))
                .pb(px(10.))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba(0x00000000), 0.),
                    linear_color_stop(rgba(0x000000cf), 1.),
                ))
                .flex()
                .flex_col()
                .items_center()
                .child(self.toolbar(window));
            root = root.child(bottom);
        }
        if !self.transitioning
            && let Some(indicator) = self.cycle_indicator(window)
        {
            root = root.child(indicator);
        }
        if self.default_banner && !empty {
            root = root.child(self.default_prompt(window));
        }
        if self.info && !empty {
            let format = self
                .path()
                .and_then(|p| p.extension())
                .unwrap_or_default()
                .to_string_lossy()
                .to_uppercase();
            let dimensions = self
                .picture
                .as_ref()
                .map(|p| format!("{} × {} pixels", p.original.0, p.original.1))
                .unwrap_or_else(|| "Video · AVFoundation".into());
            root = root.child(
                div()
                    .absolute()
                    .top(px(66.))
                    .right(px(18.))
                    .w(px(240.))
                    .p_4()
                    .rounded_lg()
                    .bg(rgba(0x27282df5))
                    .border_1()
                    .border_color(rgb(0x42434a))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child("File Information"),
                    )
                    .child(format)
                    .child(dimensions)
                    .child(format!("Zoom · {:.0}%", self.zoom * 100.))
                    .child(control(
                        "reveal",
                        "Show in Finder",
                        "Show in Finder · ⇧⌘R",
                        Reveal,
                    )),
            );
        }
        if self.help {
            root = root.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgba(0x00000070))
                    .child(self.help_panel()),
            );
        }
        root
    }
}

fn icon_control(
    id: &'static str,
    icon: &'static str,
    tooltip: &'static str,
    action: impl Action,
) -> Stateful<Div> {
    crate::ui::button(id, false, action)
        .size(px(32.))
        .px_0()
        .tooltip(move |_, cx| cx.new(|_| Tip(tooltip)).into())
        .child(
            svg()
                .path(icon)
                .size(px(16.))
                .text_color(rgb(crate::ui::TEXT)),
        )
}

fn control(
    id: &'static str,
    label: &'static str,
    tooltip: &'static str,
    action: impl Action,
) -> Stateful<Div> {
    crate::ui::button(id, id == "make-default", action)
        .tooltip(move |_, cx| cx.new(|_| Tip(tooltip)).into())
        .child(label)
}

struct Tip(&'static str);
impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded(px(10.))
            .bg(rgb(0x302a35))
            .border_1()
            .border_color(rgba(0xffffff22))
            .shadow_md()
            .text_color(rgb(crate::ui::TEXT))
            .text_size(px(12.))
            .child(self.0)
    }
}

#[cfg(debug_assertions)]
#[path = "viewer_qa.rs"]
mod qa;
