use crate::{Close, asset_path, native, viewer::About};
use gpui::*;

pub const CANVAS: u32 = 0x19191c;
pub const TEXT: u32 = 0xf3f3f5;
pub const MUTED: u32 = 0xadadb5;
pub const ACCENT: u32 = 0xb7a9bf;
pub const BUTTON_INK: u32 = 0x29262d;

fn vertical(top: u32, bottom: u32) -> Background {
    linear_gradient(
        180.,
        linear_color_stop(rgb(top), 0.),
        linear_color_stop(rgb(bottom), 1.),
    )
}

pub fn button(id: &'static str, primary: bool, action: impl Action) -> Stateful<Div> {
    let keyboard_action = action.boxed_clone();
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .h(px(36.))
        .px(px(16.))
        .flex_shrink_0()
        .rounded_full()
        .border_1()
        .border_color(rgba(if primary { 0xffffff70 } else { 0xffffff24 }))
        .bg(if primary {
            vertical(0xe2dfe5, 0xcac5d0)
        } else {
            vertical(0x3a393f, 0x303035)
        })
        .text_color(rgb(if primary { BUTTON_INK } else { TEXT }))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .shadow(vec![BoxShadow {
            color: rgba(0x00000040).into(),
            offset: point(px(0.), px(2.)),
            blur_radius: px(5.),
            spread_radius: px(0.),
        }])
        .cursor_pointer()
        .focusable()
        .tab_stop(true)
        .focus(|s| {
            s.border_color(rgb(0xf2edf6)).shadow(vec![BoxShadow {
                color: rgba(0xb7a9bf80).into(),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(3.),
            }])
        })
        .hover(move |s| {
            s.bg(if primary {
                vertical(0xeeeaf0, 0xd8d3de)
            } else {
                vertical(0x47464d, 0x3c3b42)
            })
        })
        .active(move |s| {
            s.bg(if primary {
                vertical(0xbdb7c4, 0xcac5d0)
            } else {
                vertical(0x29282e, 0x313037)
            })
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            window.dispatch_action(action.boxed_clone(), cx);
        })
        .on_key_down(move |event, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                window.dispatch_action(keyboard_action.boxed_clone(), cx);
            }
        })
}

pub fn backdrop(viewport: Size<Pixels>) -> Div {
    let columns = (f32::from(viewport.width) / 320.).ceil() as usize;
    let rows = (f32::from(viewport.height) / 280.).ceil() as usize;
    div()
        .absolute()
        .size_full()
        .overflow_hidden()
        .bg(rgb(CANVAS))
        .children((0..rows).flat_map(move |row| {
            (0..columns).map(move |column| {
                svg()
                    .path("welcome-pattern")
                    .absolute()
                    .left(px(column as f32 * 320.))
                    .top(px(row as f32 * 280.))
                    .w(px(320.))
                    .h(px(280.))
                    .text_color(rgba(0xc9c5d114))
            })
        }))
}

pub fn tab_navigation(event: &KeyDownEvent, window: &mut Window, cx: &mut App) {
    if event.keystroke.key == "tab" {
        if event.keystroke.modifiers.shift {
            window.focus_prev();
        } else {
            window.focus_next();
        }
        cx.stop_propagation();
    }
}

pub struct AboutView {
    focus: FocusHandle,
}

impl AboutView {
    pub fn open(cx: &mut App) -> anyhow::Result<WindowHandle<Self>> {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(420.), px(470.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("About Petalook".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                is_resizable: false,
                is_minimizable: false,
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus);
                    native::chrome(window, true);
                    Self { focus }
                })
            },
        )
    }
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("about")
            .key_context("About")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .font_family(".AppleSystemUIFont")
            .text_color(rgb(TEXT))
            .text_size(px(13.))
            .on_key_down(tab_navigation)
            .on_action(|_: &Close, window, _| window.remove_window())
            .on_action(|_: &About, window, _| window.activate_window())
            .child(backdrop(window.viewport_size()))
            .child(
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(44.))
                    .px(px(32.))
                    .pb(px(24.))
                    .child(img(asset_path("app-icon.png")).size(px(112.)).mb(px(16.)))
                    .child(
                        div()
                            .text_size(px(26.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Petalook"),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .px(px(10.))
                            .py(px(3.))
                            .rounded_full()
                            .border_1()
                            .border_color(rgba(0xffffff12))
                            .bg(rgba(0xffffff08))
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(
                        div()
                            .mt(px(22.))
                            .text_color(rgb(MUTED))
                            .text_center()
                            .line_height(px(20.))
                            .child("A quiet place for your images and videos."),
                    )
                    .child(div().mt(px(24.)).w(px(48.)).h(px(1.)).bg(rgba(0xefaed33a)))
                    .child(
                        div()
                            .mt(px(20.))
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child("Created by"),
                    )
                    .child(
                        div()
                            .mt(px(5.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Vahagn Mkrtchyan"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED))
                                    .child("© 2026 Vahagn Mkrtchyan"),
                            )
                            .child(button("about-done", false, Close).child("Done")),
                    ),
            )
    }
}
