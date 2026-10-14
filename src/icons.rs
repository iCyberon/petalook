use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

pub struct Icons;
impl AssetSource for Icons {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == "welcome-pattern" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/welcome-pattern.svg"
            ))));
        }
        let shape = match path {
            "open" => {
                r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v1M3 8v10a1 1 0 0 0 1 1h14l3-8H6l-3 8"/>"#
            }
            "previous" => r#"<path d="m14 6-6 6 6 6"/>"#,
            "next" => r#"<path d="m10 6 6 6-6 6"/>"#,
            "cycle" => {
                r#"<path d="M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 13 3M18 18A8 8 0 0 1 5 15"/>"#
            }
            "minus" => r#"<path d="M5 12h14"/>"#,
            "plus" => r#"<path d="M5 12h14M12 5v14"/>"#,
            "fit" => {
                r#"<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/><rect x="7" y="7" width="10" height="10" rx="1"/>"#
            }
            "fullscreen" => r#"<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>"#,
            "info" => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7v.1"/>"#,
            "play" => r#"<path d="m8 4 12 8-12 8Z"/>"#,
            "pause" => r#"<path d="M8 5v14M16 5v14"/>"#,
            "volume" => {
                r#"<path d="M11 4 6 8H3v8h3l5 4ZM15 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14"/>"#
            }
            "muted" => r#"<path d="M11 4 6 8H3v8h3l5 4ZM16 9l6 6m0-6-6 6"/>"#,
            "close" => r#"<path d="m6 6 12 12M6 18 18 6"/>"#,
            "check" => r#"<path d="m5 12 4 4L19 6"/>"#,
            _ => return Ok(None),
        };
        Ok(Some(Cow::Owned(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{shape}</svg>"#).into_bytes())))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
