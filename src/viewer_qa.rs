//! Opt-in app integration checks; excluded from release builds.
use super::*;
use core_foundation::base::TCFType;

fn assert_presentation(view: &Viewer, window: &Window, expected: &MediaPresentation) {
    let shown = view.live_presentation(window);
    assert_eq!(
        shown.bounds, expected.bounds,
        "live source geometry changed"
    );
    assert_eq!(shown.picture.is_some(), expected.picture.is_some());
    if let Some(expected) = &expected.picture {
        assert!(
            std::sync::Arc::ptr_eq(&shown.picture.unwrap().image, &expected.image),
            "live renderer replaced the source before the swipe handoff"
        );
    }
    assert_eq!(
        shown.video_frame.as_ref().map(TCFType::as_concrete_TypeRef),
        expected
            .video_frame
            .as_ref()
            .map(TCFType::as_concrete_TypeRef),
        "live renderer replaced the source video frame"
    );
}

pub(super) fn run_fit(window: &Window, cx: &mut Context<Viewer>) {
    cx.spawn_in(window, async move |this, cx| {
        Timer::after(Duration::from_secs(1)).await;
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/fixtures-aspect");
        std::fs::create_dir_all(&directory).unwrap();
        let cases = [
            ("01-wide.png", 1200, 400, [235, 157, 61, 255]),
            ("02-tall.png", 400, 1200, [54, 161, 151, 255]),
            ("03-square.png", 1200, 1200, [75, 130, 205, 255]),
            ("04-panorama.png", 1200, 120, [182, 92, 146, 255]),
        ];
        let mut paths = Vec::new();
        for (name, width, height, color) in cases {
            let path = directory.join(name);
            image::save_buffer(
                &path,
                &color.repeat(width as usize * height as usize),
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
            paths.push(path);
        }
        let maximum = Extent {
            width: 900.,
            height: 600.,
        };
        let saved_maximum = native::fit_size();
        this.update_in(cx, |view, window, cx| {
            view.auto_zoom = 0;
            view.fit_window_size = maximum;
            view.open_paths(paths, window, cx);
        })
        .unwrap();
        for index in 0..cases.len() {
            for _ in 0..100 {
                Timer::after(Duration::from_millis(30)).await;
                if this
                    .update(cx, |view, _| !view.loading && !view.transitioning)
                    .unwrap()
                {
                    break;
                }
            }
            Timer::after(Duration::from_millis(400)).await;
            this.update_in(cx, |view, window, cx| {
                assert_eq!(view.index, index);
                assert_eq!(
                    view.fit_window_size, maximum,
                    "navigation changed the maximum"
                );
                let bounds = view.media_bounds(window);
                let viewport = window.viewport_size();
                assert!(f32::from(viewport.width) <= maximum.width + 1.);
                assert!(f32::from(viewport.height) <= maximum.height + 1.);
                assert!(
                    f32::from(bounds.origin.x).abs() <= 0.6,
                    "horizontal bezel: {bounds:?}"
                );
                assert!(
                    f32::from(bounds.origin.y).abs() <= 0.6,
                    "vertical bezel: {bounds:?}"
                );
                assert!((f32::from(bounds.size.width - viewport.width)).abs() <= 1.);
                assert!((f32::from(bounds.size.height - viewport.height)).abs() <= 1.);
                eprintln!(
                    "FIT QA: {} fills {:?} within {:?}",
                    cases[index].0, viewport, maximum
                );
                if index + 1 < cases.len() {
                    view.navigate(1, window, cx);
                }
            })
            .unwrap();
        }
        this.update_in(cx, |view, window, cx| {
            view.navigate(-(view.index as i32), window, cx);
        })
        .unwrap();
        Timer::after(Duration::from_secs(1)).await;
        let enlarged = this.update_in(cx, |view, window, cx| {
            assert_eq!(view.fit_window_size, maximum);
            view.set_zoom(view.zoom * 1.25, window, cx);
            let expected = view.media_extent(window).unwrap().frame_at(view.zoom, native::available_size(window));
            assert_eq!(view.fit_window_size, expected, "zoom must update the cap");
            assert_eq!(native::fit_size(), expected, "zoom cap was not persisted");
            expected
        })
        .unwrap();
        Timer::after(Duration::from_millis(500)).await;
        this.update_in(cx, |view, window, cx| {
            assert!((f32::from(window.viewport_size().width) - enlarged.width).abs() <= 1.);
            assert!((f32::from(window.viewport_size().height) - enlarged.height).abs() <= 1.);
            view.navigate(1, window, cx);
        }).unwrap();
        Timer::after(Duration::from_secs(1)).await;
        this.update_in(cx, |view, window, cx| {
            assert_eq!(view.fit_window_size, enlarged, "portrait navigation changed zoom limits");
            assert_eq!(native::fit_size(), enlarged);
            view.set_zoom(view.zoom / 1.25, window, cx);
            let smaller = view.fit_window_size;
            assert!(smaller.width < enlarged.width && smaller.height < enlarged.height);
            assert_eq!(native::fit_size(), smaller);
            view.set_zoom(32., window, cx);
            let available = native::available_size(window);
            assert!(view.fit_window_size.width <= available.width + 1.);
            assert!(view.fit_window_size.height <= available.height + 1.);
        }).unwrap();
        Timer::after(Duration::from_millis(500)).await;
        this.update_in(cx, |view, window, _| {
            assert!((f32::from(window.viewport_size().width) - view.fit_window_size.width).abs() <= 1.);
            assert!((f32::from(window.viewport_size().height) - view.fit_window_size.height).abs() <= 1.);
            assert!(view.overflowing(window));
        }).unwrap();
        native::save_fit_size(saved_maximum);
        eprintln!("FIT QA: zoom limits persist, apply to the next aspect ratio and stop at the screen boundary");
        eprintln!("ALL FIT CHECKS PASSED");
    })
    .detach();
}

fn scroll(
    viewer: &mut Viewer,
    window: &mut Window,
    cx: &mut Context<Viewer>,
    phase: TouchPhase,
    x: f32,
) {
    viewer.scroll(
        &ScrollWheelEvent {
            position: point(px(100.), px(100.)),
            delta: ScrollDelta::Pixels(point(px(x), px(0.))),
            touch_phase: phase,
            ..Default::default()
        },
        window,
        cx,
    );
}

pub(super) fn run(window: &Window, cx: &mut Context<Viewer>) {
    cx.spawn_in(window, async move |this, cx| {
        for _ in 0..100 {
            Timer::after(Duration::from_millis(50)).await;
            if this
                .update(cx, |view, _| !view.loading && view.files.len() >= 2)
                .unwrap_or(false)
            {
                break;
            }
        }
        // Exercise the synchronous cache hit, not only the slower decoder completion path.
        let image_source = this.update(cx, |view, _| view.picture.is_some()).unwrap();
        if image_source {
            for _ in 0..100 {
                if this
                    .update(cx, |view, _| {
                        view.files
                            .get(view.index + 1)
                            .is_some_and(|path| view.cache.has(path, loader::PREVIEW_LIMIT))
                    })
                    .unwrap_or(false)
                {
                    break;
                }
                Timer::after(Duration::from_millis(20)).await;
            }
        }
        let (source, zoom, video_time, source_presentation) = this
            .update_in(cx, |view, window, cx| {
                assert!(!view.loading && view.index + 1 < view.files.len());
                if let Some(video) = &view.video {
                    video.play(false);
                }
                let original = (
                    view.index,
                    view.zoom,
                    view.video.as_ref().map(Video::time),
                    view.live_presentation(window),
                );
                let hold_distance = (f32::from(window.viewport_size().width) + 20.) * 0.25;
                scroll(view, window, cx, TouchPhase::Started, 0.);
                scroll(view, window, cx, TouchPhase::Moved, -9.);
                if image_source {
                    assert!(!view.loading, "test requires a cached incoming image");
                    let incoming = view.picture.as_ref().expect("cached target image");
                    let outgoing = view
                        .swipe_transition
                        .as_ref()
                        .unwrap()
                        .origin
                        .picture
                        .as_ref()
                        .unwrap();
                    assert!(!std::sync::Arc::ptr_eq(&incoming.image, &outgoing.image));
                }
                assert_presentation(view, window, &original.3);
                // Opacity alone cannot prevent a Metal frame overtaking WindowServer's update.
                let (count, frames) = native::transition_probe(window);
                assert_eq!(count, 1);
                assert_eq!(
                    frames[6], 0.,
                    "incoming media can flash in the outgoing live window"
                );
                eprintln!("QA: live renderer retains the source after the cached target loads");
                scroll(view, window, cx, TouchPhase::Moved, -(hold_distance - 9.));
                original
            })
            .unwrap();
        Timer::after(Duration::from_millis(400)).await;
        let held = this
            .update_in(cx, |view, window, _| {
                let swipe = view
                    .swipe_transition
                    .as_ref()
                    .expect("interactive swipe started");
                assert!(swipe.ready && swipe.released.is_none());
                assert_presentation(view, window, &source_presentation);
                let (count, frames) = native::transition_probe(window);
                assert_eq!(count, 2);
                assert!(
                    // AppKit rounds native frame coordinates to whole points.
                    (frames[3] - frames[0] - frames[1] - 20.).abs() <= 1.,
                    "unexpected window spacing: {frames:?}"
                );
                assert!((frames[2] - (1. - swipe.motion.progress())).abs() < 0.001);
                assert!((frames[5] - swipe.motion.progress()).abs() < 0.001);
                assert_eq!(frames[6], 0.);
                swipe.motion.progress()
            })
            .unwrap();
        Timer::after(Duration::from_millis(500)).await;
        this.update_in(cx, |view, window, cx| {
            assert_eq!(
                view.swipe_transition.as_ref().unwrap().motion.progress(),
                held
            );
            scroll(view, window, cx, TouchPhase::Ended, 0.);
        })
        .unwrap();
        Timer::after(Duration::from_millis(600)).await;
        this.update_in(cx, |view, window, cx| {
            assert_eq!(view.index, source);
            assert_eq!(view.zoom, zoom);
            assert!(!view.transitioning && view.swipe_transition.is_none());
            assert!(view.transition_source.is_none());
            assert_presentation(view, window, &source_presentation);
            assert_eq!(native::transition_probe(window).0, 0);
            if let Some(time) = video_time {
                let video = view.video.as_ref().unwrap();
                assert!(!video.playing());
                assert!((video.time() - time).abs() < 0.05);
            }
            // Momentum must not start another transition after release.
            scroll(view, window, cx, TouchPhase::Moved, -500.);
            assert_eq!(view.index, source);
            eprintln!(
                "QA: paused swipe, cancellation, source state and momentum suppression passed"
            );
            scroll(view, window, cx, TouchPhase::Started, 0.);
            scroll(
                view,
                window,
                cx,
                TouchPhase::Moved,
                -(f32::from(window.viewport_size().width) + 20.) * 0.6,
            );
        })
        .unwrap();
        Timer::after(Duration::from_millis(300)).await;
        this.update_in(cx, |view, window, cx| {
            scroll(view, window, cx, TouchPhase::Ended, 0.)
        })
        .unwrap();
        Timer::after(Duration::from_millis(600)).await;
        this.update_in(cx, |view, window, cx| {
            assert_eq!(view.index, source + 1);
            assert!(!view.transitioning && view.swipe_transition.is_none());
            assert!(view.transition_source.is_none());
            if let Some(picture) = &view.picture {
                assert!(std::sync::Arc::ptr_eq(
                    &view.live_presentation(window).picture.unwrap().image,
                    &picture.image,
                ));
            }
            eprintln!("QA: committed swipe passed");
            scroll(view, window, cx, TouchPhase::Started, 0.);
            scroll(view, window, cx, TouchPhase::Moved, 100.);
            scroll(view, window, cx, TouchPhase::Moved, -110.);
            scroll(view, window, cx, TouchPhase::Ended, 0.);
        })
        .unwrap();
        Timer::after(Duration::from_millis(600)).await;
        this.update_in(cx, |view, window, cx| {
            assert_eq!(view.index, source + 1);
            assert!(!view.transitioning && view.swipe_transition.is_none());
            eprintln!("QA: reversed swipe and release before preview readiness passed");
            view.navigate(-1, window, cx);
            view.navigate(1, window, cx);
            view.navigate(-1, window, cx);
        })
        .unwrap();
        Timer::after(Duration::from_millis(1000)).await;
        this.update(cx, |view, _| {
            assert_eq!(view.index, source);
            assert!(!view.transitioning && view.pending_navigation.is_none());
            eprintln!("QA: rapid arrow navigation passed");
            view.files = vec![view.files[source].clone(), view.files[source + 1].clone()];
            view.index = 0;
        })
        .unwrap();

        // Both wrap directions, including arrows queued during an active transition.
        for (direction, index, boundary) in [
            (-1, 1, Boundary::Beginning),
            (1, 0, Boundary::End),
        ] {
            this.update_in(cx, |view, window, cx| {
                let source = view.live_presentation(window);
                view.navigate(direction, window, cx);
                assert_presentation(view, window, &source);
            }).unwrap();
            for _ in 0..150 {
                Timer::after(Duration::from_millis(20)).await;
                if this.update(cx, |view, _| !view.loading && !view.transitioning).unwrap() { break; }
            }
            this.update_in(cx, |view, window, _| {
                assert!(!view.transitioning && view.index == index);
                assert_eq!(view.cycle_notice.map(|notice| notice.0), Some(boundary));
                assert_eq!(native::snapshot_count(window), 0);
            }).unwrap();
        }
        // A new gesture arriving in the same event batch must not race native rollback.
        for _ in 0..24 {
            this.update_in(cx, |view, window, cx| {
                scroll(view, window, cx, TouchPhase::Started, 0.);
                scroll(view, window, cx, TouchPhase::Moved, 9.);
                let epoch = view.transition_epoch;
                scroll(view, window, cx, TouchPhase::Ended, 0.);
                assert_eq!(view.index, 0);
                assert!(view.transitioning, "accepted input before native handoff finished");
                view.transition_completed(epoch, window, cx);
                assert!(view.transitioning, "stale completion finished a newer transition");
                scroll(view, window, cx, TouchPhase::Started, 0.);
                scroll(view, window, cx, TouchPhase::Moved, -100.);
                assert_eq!(view.index, 0);
            }).unwrap();
            for _ in 0..100 {
                Timer::after(Duration::from_millis(10)).await;
                if this.update(cx, |view, _| !view.transitioning).unwrap() { break; }
            }
            this.update_in(cx, |view, window, _| {
                assert!(!view.transitioning && view.swipe_transition.is_none());
                assert!(view.cycle_notice.is_none(), "cancelled wrap showed a completion cue");
                assert_eq!(native::snapshot_count(window), 0, "orphan snapshot after cancellation");
            }).unwrap();
        }
        for (delta, index, boundary) in [
            (1., 1, Boundary::Beginning),
            (-1., 0, Boundary::End),
        ] {
            this.update_in(cx, |view, window, cx| {
                scroll(view, window, cx, TouchPhase::Started, 0.);
                scroll(view, window, cx, TouchPhase::Moved, delta * (f32::from(window.viewport_size().width) + 20.) * 0.65);
                scroll(view, window, cx, TouchPhase::Ended, 0.);
            }).unwrap();
            for _ in 0..150 {
                Timer::after(Duration::from_millis(20)).await;
                if this.update(cx, |view, _| !view.loading && !view.transitioning).unwrap() { break; }
            }
            this.update_in(cx, |view, window, _| {
                assert!(!view.transitioning && view.index == index);
                assert_eq!(view.cycle_notice.map(|notice| notice.0), Some(boundary));
                assert_eq!(native::snapshot_count(window), 0);
            }).unwrap();
        }
        this.update_in(cx, |view, window, cx| {
            view.navigate(1, window, cx);
            view.navigate(1, window, cx);
        }).unwrap();
        Timer::after(Duration::from_millis(1000)).await;
        this.update_in(cx, |view, window, _| {
            assert!(!view.transitioning && view.index == 0);
            assert_eq!(view.cycle_notice.map(|notice| notice.0), Some(Boundary::End));
            assert_eq!(native::snapshot_count(window), 0);
            eprintln!("QA: wraps, cancelled wraps, 24 rapid handoffs and stale completion rejection passed; ALL SWIPE CHECKS PASSED");
        }).unwrap();
    })
    .detach();
}
