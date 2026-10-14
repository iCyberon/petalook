mod geometry;
mod icons;
mod library;
mod loader;
mod native;
mod navigation;
mod startup;
mod swipe;
mod ui;
mod updater;
mod viewer;

use gpui::*;
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use viewer::*;

actions!(
    petalook,
    [
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Close,
        CheckForUpdates
    ]
);

fn main() {
    startup::begin();
    let paths: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter(|p| !p.to_string_lossy().starts_with("-psn_"))
        .map(PathBuf::from)
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect();
    let loader = loader::start();
    loader.requests.prime(&paths);
    let app = Application::new().with_assets(icons::Icons);
    startup::mark("application_created");
    let (open_sender, open_receiver) = async_channel::unbounded();
    let launch_paths = Rc::new(RefCell::new(Some(paths)));
    let early_paths = launch_paths.clone();
    let early_requests = loader.requests.clone();
    app.on_open_urls(move |urls| {
        startup::mark("open_urls");
        let paths = urls
            .iter()
            .filter_map(|u| url::Url::parse(u).ok()?.to_file_path().ok())
            .collect::<Vec<_>>();
        if !paths.is_empty() {
            let mut initial = early_paths.borrow_mut();
            if initial.is_some() {
                early_requests.prime(&paths);
                *initial = Some(paths);
            } else {
                let _ = open_sender.try_send(paths);
            }
        }
    });
    app.run(move |cx| {
        startup::mark("launch_callback");
        let paths = launch_paths.borrow_mut().take().unwrap_or_default();

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
        cx.on_action(|_: &CheckForUpdates, _| updater::check());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("cmd-o", Open, None),
            KeyBinding::new("cmd-w", Close, None),
            KeyBinding::new("escape", Close, Some("About")),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("right", Next, Some("Viewer")),
            KeyBinding::new("left", Previous, Some("Viewer")),
            KeyBinding::new("down", Next, Some("Viewer")),
            KeyBinding::new("up", Previous, Some("Viewer")),
            KeyBinding::new("cmd-0", Fit, Some("Viewer")),
            KeyBinding::new("cmd-1", ActualSize, Some("Viewer")),
            KeyBinding::new("cmd-=", ZoomIn, Some("Viewer")),
            KeyBinding::new("cmd-+", ZoomIn, Some("Viewer")),
            KeyBinding::new("cmd--", ZoomOut, Some("Viewer")),
            KeyBinding::new("space", PlayPause, Some("Viewer")),
            KeyBinding::new("ctrl-cmd-f", Fullscreen, None),
            KeyBinding::new("escape", Escape, Some("Viewer")),
            KeyBinding::new("cmd-i", ToggleInfo, Some("Viewer")),
            KeyBinding::new("cmd-shift-r", Reveal, Some("Viewer")),
            KeyBinding::new("alt-left", SeekBackward, Some("Viewer")),
            KeyBinding::new("alt-right", SeekForward, Some("Viewer")),
            KeyBinding::new("m", Mute, Some("Viewer")),
            KeyBinding::new("cmd-/", Shortcuts, Some("Viewer")),
        ]);
        cx.set_menus(vec![
            Menu {
                name: "Petalook".into(),
                items: vec![
                    MenuItem::action("About Petalook", About),
                    MenuItem::action("Check for Updates…", CheckForUpdates),
                    MenuItem::separator(),
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Hide Petalook", Hide),
                    MenuItem::action("Hide Others", HideOthers),
                    MenuItem::action("Show All", ShowAll),
                    MenuItem::separator(),
                    MenuItem::action("Quit Petalook", Quit),
                ],
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("Open…", Open),
                    MenuItem::action("Show in Finder", Reveal),
                    MenuItem::action("Default Viewer…", DefaultViewer),
                    MenuItem::separator(),
                    MenuItem::action("Close Window", Close),
                ],
            },
            Menu {
                name: "View".into(),
                items: vec![
                    MenuItem::action("Next File", Next),
                    MenuItem::action("Previous File", Previous),
                    MenuItem::separator(),
                    MenuItem::action("Zoom In", ZoomIn),
                    MenuItem::action("Zoom Out", ZoomOut),
                    MenuItem::action("Reset Zoom", Fit),
                    MenuItem::submenu(Menu {
                        name: "Auto Zoom".into(),
                        items: vec![
                            MenuItem::action("Fit to Screen", FitScreen),
                            MenuItem::action("Fit to Window", FitWindow),
                        ],
                    }),
                    MenuItem::action("Actual Size", ActualSize),
                    MenuItem::separator(),
                    MenuItem::action("File Information", ToggleInfo),
                ],
            },
            Menu {
                name: "Playback".into(),
                items: vec![
                    MenuItem::action("Play / Pause", PlayPause),
                    MenuItem::action("Back 5 Seconds", SeekBackward),
                    MenuItem::action("Forward 5 Seconds", SeekForward),
                    MenuItem::action("Mute / Unmute", Mute),
                ],
            },
            Menu {
                name: "Window".into(),
                items: vec![MenuItem::action("Minimize", Minimize)],
            },
            Menu {
                name: "Help".into(),
                items: vec![MenuItem::action("Keyboard Shortcuts", Shortcuts)],
            },
        ]);
        startup::mark("menus_ready");
        let bounds = Bounds::centered(None, native::saved_size(), cx);
        startup::mark("preferences_ready");
        let window = cx
            .open_window(
                WindowOptions {
                    is_resizable: false,
                    is_movable: false,
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1.), px(1.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Petalook".into()),
                        appears_transparent: true,
                        // AppKit owns placement in both active and inactive states.
                        traffic_light_position: None,
                    }),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Viewer::new(window, cx, paths, open_receiver, loader)),
            )
            .expect("open viewer window");
        let _ = window;
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}

pub fn asset_path(name: &str) -> PathBuf {
    if let Ok(executable) = std::env::current_exe()
        && let Some(macos) = executable.parent()
    {
        let bundled = macos.join("../Resources").join(name);
        if bundled.exists() {
            return bundled;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(name)
}
