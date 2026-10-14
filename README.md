<p align="center">
  <img src="assets/app-icon.png" width="160" alt="Petalook app icon">
</p>

# Petalook

A quiet, fast image and video viewer for macOS. Written in Rust with [GPUI](https://www.gpui.rs/), inspired by [LilyView](https://lilyviewapp.com/).

Open a picture and let the interface disappear. Pinch to grow the image and its window together. Swipe between neighboring files, with the transition following your fingers.

**[Download the latest release](https://github.com/icyberon/petalook/releases/latest)** · **[Report an issue](https://github.com/icyberon/petalook/issues)**

## Features

- Image-first startup, background folder scanning, and neighboring-image preloading.
- Trackpad swipes and arrow-key navigation, with continuous cycling through a folder.
- Frame-shaped zoom: the window fits the image, including when aspect ratios change.
- Remembered Fit to Window limits and a Fit to Screen mode.
- Drag the image to move its window, with gentle snapping near the screen center.
- Controls that appear near the bottom edge: Open, filename / Show in Finder, and Info.
- Video playback with audio, seeking, and mute controls.
- Signed background updates through Sparkle, installed when you quit and ready on the next launch.

## Install

Download the release ZIP, extract **Petalook.app**, and move it to **Applications**. The current release is for **Apple Silicon Macs running macOS 12 or later**. Windows and Linux ports are planned; they are not supported yet.

Open files from Finder's **Open With** menu, drop images, videos, or folders into the window, or press **⌘O**. Opening a single file includes supported neighbors from the same folder in natural filename order.

The optional default-viewer setup uses macOS confirmation dialogs. macOS may ask separately for each content type. Nothing changes until you request it and approve the system prompts.

## Controls

| Action | Shortcut / gesture |
| --- | --- |
| Open files or folders | ⌘O / drag and drop |
| Previous / next | ← / → / horizontal two-finger swipe |
| Zoom | Pinch / ⌘+ / ⌘− |
| Move the window | Drag anywhere on the image |
| Pan a zoomed image | Two-finger scroll |
| Auto Zoom / actual size | ⌘0 / ⌘1 |
| Play / pause video | Space |
| Seek backward / forward | ⌥← / ⌥→ |
| Mute | M |
| File information | ⌘I |
| Show in Finder | ⇧⌘R / click the filename |
| All shortcuts | ⌘/ |

## Supported media

Images: JPEG, PNG, HEIC/HEIF, TIFF, BMP, GIF, WebP, ICO, and AVIF where supported by macOS ImageIO.

Videos: MP4, MOV, and M4V with codecs supported by AVFoundation, including H.264 and HEVC.

Animated images currently show their first frame. Decoded image detail is limited to 8,192 pixels on the longest edge. RAW, HDR display, image editing, and tiled gigapixel rendering are not supported. Custom GPUI controls do not yet provide a complete VoiceOver accessibility tree; native menus provide the available commands.

## Build from source

Requires macOS, stable Rust (tested with 1.93), Python 3.11+, and full Xcode with its macOS SDK and Metal toolchain.

```sh
git clone https://github.com/icyberon/petalook.git
cd petalook
bash scripts/fetch-sparkle.sh
cargo test --locked
bash scripts/bundle.sh
open dist/Petalook.app
```

Local builds are signed ad hoc. See [development notes](docs/development.md) for architecture, regression tests, startup measurements, and Developer ID signing / notarization. The bundle identifier is `am.mkrtchyan.petalook`. Existing SunflowerView viewing preferences migrate automatically on first launch.

## Releases

Updates are hosted on [GitHub Releases](https://github.com/icyberon/petalook/releases). The release configuration contains only the public update-signing key; the private key stays in the maintainer's Keychain.

To publish a new version, update `Cargo.toml` and increment `build_number` in `release/updates.json`, then run:

```sh
PETALOOK_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
PETALOOK_NOTARY_PROFILE="your-notary-profile" \
python3 scripts/package-release.py
```

Upload the generated ZIP and `appcast.xml` together to the matching versioned GitHub release. Release archives are architecture-specific. Packaging does not publish automatically.

## Credits and license

Created by **Vahagn Mkrtchyan**. Licensed under [MIT](LICENSE).

Built with [GPUI](https://www.gpui.rs/) and [Sparkle](https://sparkle-project.org/). The dahlia icon was generated and refined for this project; see [artwork notes](assets/icon-prompt.md).
