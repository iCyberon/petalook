# Petalook development notes

A native macOS image and video viewer written in Rust with GPUI. Inspired by [LilyView](https://lilyviewapp.com/): open a file, browse its neighbors, and let the interface disappear.

## Run

Open `dist/Petalook.app`. You can drag the app into Applications, open files from its File menu, drop files or folders into its window, or choose it in Finder’s **Open With** menu. The first-run banner offers to make Petalook your default viewer. File associations change only after you click **Make Default** and accept any macOS consent prompts. Dismiss the banner with **×**; that choice survives restarts. **File → Default Viewer…** brings it back.

```sh
bash scripts/fetch-sparkle.sh
cargo run --release -- /path/to/photo.jpg
cargo run --release -- /path/to/folder
```

Opening one file includes supported files in the same folder, ordered naturally (`photo2` before `photo10`). Opening multiple files creates a selection from those files. Folder scanning is nonrecursive.

## Controls

| Action | Keyboard / gesture |
| --- | --- |
| Open files or folders | ⌘O / drop into the window |
| Previous / next file | ← / →, ↑ / ↓, horizontal two-finger swipe |
| Zoom | Pinch, ⌘+ / ⌘−, ⌘-scroll |
| Smart zoom | Double-click / trackpad smart magnify |
| Move window | Drag anywhere on the image; snaps near the screen center |
| Pan while zoomed | Two-finger scroll / scrollbars |
| Reset to selected Auto Zoom mode / actual size | ⌘0 / ⌘1 |
| Play / pause video | Space |
| Seek backward / forward 5 seconds | ⌥← / ⌥→ |
| Mute / unmute | M |
| Fullscreen | ⌃⌘F |
| File information | ⌘I |
| Show in Finder | ⇧⌘R |
| Keyboard shortcuts | ⌘/ |
| Dismiss panel / exit fullscreen / reset zoom | Esc |

Open and Info flank a centered filename on a soft bottom gradient. Clicking the filename reveals the current file in Finder. The controls start hidden and appear only when the pointer approaches the bottom of the window; moving away hides them. Keyboard navigation does not reveal the controls. Native traffic lights hide after one second and return when the pointer approaches the top 44 points of the window; there is no titlebar strip. At folder boundaries, navigation wraps with a brief centered cycle indicator. Commands are also available in native macOS menus.

### Frame and zoom

**View → Auto Zoom** chooses how a newly opened file is presented:

- **Fit to Screen** shapes the window around the media, constrained to the current display’s usable area. Automatic fitting does not enlarge media beyond its actual pixel size. This is the initial default.
- **Fit to Window** remembers a maximum width and height, then shapes each window to its image within those limits. With a 900 × 600 limit, a 3:1 image appears in a 900 × 300 window and a 1:3 image in a 200 × 600 window, without letterboxing. Pinching or using ⌘+ / ⌘− sets new limits from the resulting frame size, including when zooming out. Navigation preserves these limits; edge resizing is disabled. The limits are saved separately from the current image’s frame, so changing aspect ratios or restarting the app does not progressively shrink the viewing area. Window position and the selected mode also survive restarts. Disconnected-display coordinates are brought onto an available display.

Pinching or pressing ⌘+ / ⌘− scales the whole frame first, preserving its proportions. The frame stops growing at the usable screen boundary and stops shrinking at an image-shaped minimum inside 300 × 180 points (or the image’s native size, when smaller). Aspect ratio takes priority over an independent minimum width or height. Further zoom changes the media inside the frame. Overflow adds draggable horizontal/vertical scrollbars; two-finger scrolling pans the image. Dragging anywhere on the image always moves the whole window, even while zoomed in. Near the current display’s usable center, each axis snaps within 12 points and releases beyond 24 points to prevent jitter. Window moves preserve the image size and fit limits. Toolbar controls and scrollbars keep their normal click/drag behavior. Fullscreen zoom leaves the remembered window limits unchanged. ⌘0 resets to the selected Auto Zoom mode; ⌘1 shows one image pixel per display pixel.

Previous/next navigation uses two temporary native windows. Their bitmaps have transparent rounded corners, so the image keeps its rounded shape throughout movement, scaling and fading. The outgoing frame slides away, fades and shrinks to 94%; the incoming frame slides in, fades up and grows to full size. A 20-point gap keeps the frames from overlapping, including for different aspect ratios. The live GPUI window stays at its destination size underneath. At completion, its Metal layer is explicitly redrawn and committed under the snapshots before it becomes visible; a native acknowledgement then releases navigation. This also applies when a gesture rolls back, because macOS stops the display loop of a transparent window. Transition generations reject callbacks belonging to an earlier gesture, and incoming bitmap updates reuse their existing snapshot window. Navigation cycles from last to first and first to last while preserving the requested slide direction. A centered circular-arrow cue fades in and out for 1.1 seconds after a committed wrap; cancelled swipes do not show it. The first/last neighbours are prefetched just like other adjacent files. Arrow-key transitions take 280 ms. Repeated arrows coalesce to the latest requested file rather than stacking animations.

Two-finger horizontal swipes directly control progress. Pausing fingers holds both frames at that position; reversing moves them back. On release, crossing 42% completes the transition, as does a deliberate forward flick; shorter swipes return to the original media. Cancellation restores the original window frame, zoom and paused/playing video state. Momentum events after release cannot trigger extra navigation. Escape cancels an active swipe. Scroll phases are delivered at the native window level, so resizing to a different aspect ratio cannot drop a gesture when the pointer falls outside the new frame. Transition images accept clicks and return focus to the viewer; switching apps or losing window focus cancels an unfinished swipe. While zoomed beyond the frame, scrolling pans the image instead. In full screen or with macOS Reduce Motion enabled, swipes navigate on release without window animation.

The temporary windows use decoded image pixels or video frames, so they do not require screen-recording permission. A pending decode keeps the outgoing image visible, and no animation runs while fingers are holding still. Each temporary window receives a complete raster image before it is shown, using an [AppKit hosting layer](https://developer.apple.com/documentation/appkit/nsview/wantslayer); the handoff does not depend on a deferred draw callback. The outgoing snapshot is ordered and the live viewer is hidden before loading the next selection. WindowServer opacity changes and Metal presentation are not an atomic operation: a regression recording showed a single target frame at 3.067 seconds despite a zero-opacity compositor probe. The live renderer therefore retains the source image/video frame and its geometry until handoff, independently of the pending target. This shares existing decoded storage and adds no decode or pixel copy. Only completion or cancellation releases the retained presentation; the target is then drawn under the snapshots before the host is revealed. The incoming window is positioned, scaled and faded to the current gesture progress before being ordered on screen. Temporary windows disable [AppKit’s automatic ordering animations](https://developer.apple.com/documentation/appkit/nswindow/animationbehavior-swift.property), since the app controls their movement. Queued cleanup retains the old snapshot until its replacement is visible and cannot reveal the live viewer during a newer transition.

## Media

- Images: JPEG, PNG, HEIC/HEIF, TIFF, BMP, GIF, WebP, ICO, and AVIF when supported by the installed macOS ImageIO codecs. EXIF orientation and embedded color profiles are handled by ImageIO, with output converted to sRGB.
- Videos: MP4, MOV, and M4V with macOS-supported codecs, including H.264 and HEVC. Playback includes audio, pause, seek, mute, and automatic orientation for rotated video tracks.

Current limits: animated images show their first frame; image detail is capped at 8,192 pixels on the longest edge; this build does not provide tiled gigapixel rendering, RAW support, HDR display, arbitrary video codecs, image rotation controls, editing, or a photo library. GPUI 0.2.2 does not expose the custom canvas controls as a complete VoiceOver accessibility tree; native menus remain available. This is a working initial implementation, not full LilyView feature parity.

## Build

Requirements: macOS, a current stable Rust toolchain (tested with 1.93), full Xcode with its macOS SDK and Metal toolchain, and the Xcode command-line tools selected.

```sh
# If Xcode reports a missing Metal compiler:
xcodebuild -downloadComponent MetalToolchain
xcrun --kill-cache

cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cc -Wall -Wextra -Werror tests/slide_geometry.c -o /tmp/petalook-slide-tests
/tmp/petalook-slide-tests
xcrun clang -fobjc-arc -Wall -Wextra -Werror -Wno-deprecated-declarations tests/window_handoff.m -framework AppKit -framework QuartzCore -framework CoreImage -framework UniformTypeIdentifiers -framework AVFoundation -framework ImageIO -framework CoreVideo -framework CoreMedia -o /tmp/petalook-handoff-tests
/tmp/petalook-handoff-tests
bash scripts/bundle.sh
open dist/Petalook.app
```

The bundle script creates an app for the current Mac’s architecture and signs it ad hoc for local use. Distribution to other Macs requires a Developer ID signature and notarization. The generated app icon and original sunflower are bundled resources, so the app works without the source checkout.

## Implementation

- `src/viewer.rs`: GPUI window, commands, gestures, media state, and overlays.
- `src/geometry.rs`: frame sizing, aspect constraints, and zoom limits.
- `src/swipe.rs`: finger displacement, release thresholds, reversal and flick decisions.
- `native/slide_geometry.h`: shared non-overlapping window motion geometry.
- `src/viewer_qa.rs`: opt-in integration checks, excluded from release builds.
- `src/icons.rs`: embedded vector control icons.
- `src/library.rs`: supported media classification and natural folder ordering.
- `src/loader.rs`: a foreground preview worker and a separate utility-priority worker for zoom detail and neighboring-file prefetch, plus a five-entry / 320 MiB decoded image cache. Queued requests coalesce; an in-flight result can satisfy a newer request for the same image without decoding it twice. An active ImageIO call cannot be interrupted, but background work cannot occupy the preview worker. Eviction also removes uploaded images from GPUI’s atlas. The cache budget excludes native decoder working memory and GPU allocations.
- `src/native.rs`: Rust ownership wrappers for the macOS bridge.
- `native/media.m`: ImageIO decoding, AVFoundation playback, and native magnification events.
- `native/window.m`: AppKit window bounds/chrome, temporary transition windows, app preferences, and the [native default-application API](https://developer.apple.com/documentation/appkit/nsworkspace/setdefaultapplication(at:toopen:completion:)). AppKit resizing is scheduled after the GPUI update finishes so resize callbacks can update the Metal drawable safely. Application state and controls remain Rust/GPUI.

Preferences use the `am.mkrtchyan.petalook` application domain. On first launch, missing viewer settings are copied once from the former `app.sunflowerview.viewer` domain; existing Petalook settings win, and updater settings are never imported. The viewer settings are: `autoZoom`, `windowFrame`, `fitWindowMaximum`, and `defaultBannerDismissed`. Existing installations seed the independent fit limits from their previously saved window size.

Image workers start before GPUI/macOS initialization. Command-line images decode immediately; Finder file events start decoding as soon as macOS delivers them, even before the window exists. A completed preview is used in the first render. The selected file gets priority over folder enumeration, which begins after its first rendered frame. Default-viewer checks, recent-document bookkeeping and neighboring-file preloads also follow the image. The bundled app uses its existing Dock icon instead of synchronously setting it again (about 85 ms saved in the measured baseline). A failed sibling scan does not hide a successfully opened image. Photos initially decode to a 3,072-pixel preview; zooming requests more detail on the background worker so it cannot block the next image. Old results are rejected by generation number, so rapid navigation cannot replace the current image with an older request. Static images do not run a continuous render loop. Videos use CoreVideo NV12 buffers rendered through GPUI’s Metal surface, avoiding a CPU copy for every frame. Paused videos check for seek results at a reduced rate.

### Startup measurements

Set `PETALOOK_STARTUP_TRACE=1` for timestamps covering macOS initialization, image decoding, rendering and deferred setup. `first_media_frame` is the GPUI callback after the first render containing an image or video frame. It includes image rendering, unlike the previous decode-ready log, but is not a measurement of physical display latency. Normal launches do not print startup logs.

Measured on a MacBook Pro with M5 Max, 128 GB RAM and macOS 27.0. Each entry is the median of five release-build launches after one discarded warmup. Both builds used the same fixtures and instrumentation. The parent process clock includes process creation/dyld and ends when the image-frame marker arrives. These are **warm-cache direct executable launches**, not cold starts or Finder timings.

| Fixture | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| 1,254 × 1,254 JPEG | 275 ms | 135 ms | 51% |
| 1,254 × 1,254 PNG | 280 ms | 142 ms | 49% |
| 6,000 × 4,000 JPEG | 358 ms | 204 ms | 43% |
| 6,000 × 4,000 PNG | 443 ms | 269 ms | 39% |
| 6,000 × 4,000 HEIC | 322 ms | 198 ms | 39% |

MacOS file delivery was checked separately using `open -n -a` without command-line image arguments. In a five-run batch with two seconds between launches, end-to-end medians were 376 ms for the small PNG and 467 ms for the large JPEG. Earlier batches varied substantially (253 ms / 705 ms), so these are observations rather than a latency guarantee. The app cannot decode a Finder request before macOS delivers its URL. Files also open correctly in the already-running app.

Raw measurements and methodology are in [`benchmarks/startup-2026-10-14.json`](../benchmarks/startup-2026-10-14.json). The measured pre-updater app bundle was approximately **12 MiB**; the current bundle including Sparkle is approximately **15 MiB**. To repeat the measurements:

```sh
swift scripts/make-image-fixtures.swift target/fixtures-large
bash scripts/bundle.sh
python3 scripts/benchmark-startup.py dist/Petalook.app/Contents/MacOS/Petalook \
  target/fixtures-large/large.jpg target/fixtures-large/large.png target/fixtures-large/large.heic \
  --runs 5 --output target/startup.json
python3 scripts/benchmark-startup.py dist/Petalook.app target/fixtures-large/large.jpg \
  --launch-services --runs 5 --output target/startup-finder.json
```

The benchmark creates short-lived test instances and terminates only those instances. It does not quit an existing viewer or change file associations.

## Updates and releases

The macOS updater uses Sparkle 2.10.0. It is loaded dynamically after the first rendered image or welcome screen, so framework initialization and network requests do not delay the first draw. On launch it checks in the background (unless automatic checks have been disabled in Sparkle preferences), downloads signed updates, and stages them until normal application termination. The new version is ready on the next launch. A pending update does not interrupt the viewer or automatically relaunch it. **Petalook → Check for Updates…** performs a manual check or reports that an update is ready.

`release/updates.json` holds the GitHub repository, public Ed25519 signing key, Keychain account, and monotonically increasing build number. Until the repository and key are set, background checks stay inactive; the manual action explains that the build has no update channel. The app never contacts a placeholder server. The production feed is hosted on GitHub Releases at `https://github.com/icyberon/petalook/releases/latest/download/appcast.xml`.

Connect an existing public GitHub repository once:

```sh
python3 scripts/configure-updates.py https://github.com/icyberon/petalook
```

This creates or reuses the app's dedicated signing key in macOS Keychain. Only the public key is written into the project. Keep the private key safe; do not commit or upload it. `--public-key` can supply an existing signing public key, but packaging requires the matching private key in the configured Keychain account. Private GitHub repositories require a separate authenticated distribution design; do not embed a GitHub token in the app.

For each release, increment `version` in `Cargo.toml` and `build_number` in `release/updates.json`, then run:

```sh
python3 scripts/package-release.py
```

The script bundles the app, creates an archive, and signs it with Sparkle's `generate_appcast`. It produces a ZIP and `appcast.xml` under `dist/releases/vVERSION/`; it does not publish them. Upload both assets together to that GitHub release and make it the latest stable release. The app's permanent feed URL is `https://github.com/icyberon/petalook/releases/latest/download/appcast.xml`; archive links reference the specific version tag. Build and marketing versions are copied into the bundle from the release configuration, so successive updates are compared by build number.

Local bundles use ad-hoc code signing. For public distribution, set `PETALOOK_SIGNING_IDENTITY` to your Developer ID identity and `PETALOOK_NOTARY_PROFILE` to an existing notarytool Keychain profile. The packaging script then notarizes and staples the app before creating the update archive. The build signs Sparkle helpers before the framework and outer app, preserving framework symlinks. Update archives require a valid Ed25519 signature before extraction. The official Sparkle dependency is pinned and checksum-verified by `scripts/fetch-sparkle.sh`.

`python3 tests/test_updates.py` runs a real download/install cycle against a localhost feed using disposable bundles and a temporary signing key, then verifies that an invalid signature cannot replace the installed app. The test alone permits loopback HTTP; production feeds require HTTPS. `python3 tests/test_release_config.py` checks release URL/key validation and incomplete-configuration handling.

### Other desktop platforms

macOS remains the supported release target. The shared Rust UI, navigation, caching, and geometry can be reused for Windows/Linux, but AppKit window behavior, ImageIO decoding, AVFoundation playback, gestures, file associations, and packaging need platform implementations. `src/updater.rs` is a small platform boundary; Sparkle lives entirely in the macOS backend. Windows/Linux should add their own installation backend (and respect Linux package-manager ownership) without changing viewer actions. No untested Windows/Linux binaries are produced by the current scripts.

## Verification

Twenty Rust regression tests cover cyclic navigation (including empty/single-file lists), natural ordering, folder selection/filtering, cache eviction, decode bounds, transparency, BGRA channel order, image row orientation, invalid files, frame growth limits, minimum sizing, extreme aspect ratios, swipe hold/release thresholds, reversal and flick behavior. Loader tests also verify that a blocked preload or zoom-detail decode cannot block a selected preview, that superseded results are discarded, and that an in-flight decode is reused for the same file. Native geometry checks cover 18,018 frames across both directions and different aspect ratios, asserting endpoints, scaling proportions and the gap between frames. The native handoff regression test checks WindowServer opacity before returning to the input handler, incoming-window placement at its first ordering call, rounded-corner pixel transparency, image orientation, layer resizing, overlapping cleanup requests and window teardown. Input regressions cover scroll phases outside the resized viewport, events addressed to a transition image, click recovery, focus loss and observer cleanup. Magnetic-drag checks cover independent axes, capture/release thresholds, negative display coordinates, unchanged window size and persistence without changing zoom limits. Edge-resize disabling is checked alongside programmatic frame resizing. It also checks narrow native windows and that saving a portrait frame preserves the independent maximum width and height. Its compositor assertion fails with the previous handoff, even though AppKit reports zero opacity. The app integration check also exercises a preloaded incoming image with a nine-point initial finger movement. A regression that failed on the previous build verifies that the live renderer still selects the source after the cached target has loaded; it also checks source geometry and video frame identity while held, source restoration on cancellation, target presentation after commitment, and retained source presentation for arrow navigation. The compositor opacity probe remains a separate check, not proof that no target frame can flash. A deferred-layer regression fails against the old handoff when it reveals a stale frame; the fixed handoff draws before revealing and acknowledges only after snapshot cleanup. Other checks cover incoming surface reuse, 24 rapid rollback/restart attempts per image/video run, stale completion rejection, both wrap directions, cancelled wrap cues and queued navigation through a boundary.

Native UI checks covered fresh-process and running-app Finder delivery, 24-megapixel HEIC opening, navigation immediately after requesting zoom detail, folder-to-video opening, JPEG/transparent PNG navigation, rapid direction changes, both Auto Zoom menu options, frame resizing to screen limits, overflow scrollbars, titlebar hiding, banner dismissal across restarts, portrait H.264 playback/pause with compact controls, and remembered Fit to Window limits across changes of image shape. The fit integration check covers 3:1, 1:3, square and 10:1 images, verifies that media fills each native viewport, and checks that navigation preserves the limits while explicit zoom updates and persists them, capped by the usable display size. Physical trackpad pinch/swipe and drag-and-drop still need hands-on verification. The Make Default action is implemented but was deliberately not activated during QA, so the developer’s file associations remain unchanged.

Set `PETALOOK_TRACE=1` when launching from a terminal to log transition completion and overlay state. Debug builds support `PETALOOK_TRANSITION_MS=2000` for slow-motion visual inspection.

The opt-in app integration check exercises the same scroll handler with started/moved/ended phases. It verifies held progress, native window spacing and opacity, cancellation with video state restoration, commit, reversal before preview readiness, momentum suppression, cleanup and rapid arrow navigation:

```sh
bash scripts/bundle.sh debug
PETALOOK_SWIPE_QA=1 dist/Petalook.app/Contents/MacOS/Petalook target/fixtures/photo1.jpg
# Repeat from clip11.mp4 to also cover video-state restoration.
```

Set `PETALOOK_FIT_QA=1` instead to generate aspect-ratio fixtures under `target/fixtures-aspect` and run the bounding-box fit checks.

Run these checks without interacting with the test app until `ALL SWIPE CHECKS PASSED` or `ALL FIT CHECKS PASSED` appears. These injected app events test the interaction state machine and native animation windows; they do not replace a physical trackpad check.

Generate a local 12-second video fixture with Apple’s tools:

```sh
swift scripts/make-fixtures.swift target/fixtures
```

## Artwork

`assets/app-icon.png` is the new pink dahlia icon, generated with the built-in image-generation tool: a symmetrical flower, sage photo inset, pearl-white bezel and a glass loupe. The full generation and refinement prompts are in `assets/icon-prompt.md`. The previous dark sunflower icon is preserved as `assets/app-icon-charcoal.png`. The welcome screen and About panel use the current app icon. The original supplied sunflower is preserved in `assets/sunflower.png`.

The generated source is 1,254 × 1,254 pixels with transparent margins. The bundle script derives the standard macOS icon sizes, including Retina variants, into `AppIcon.icns`.
