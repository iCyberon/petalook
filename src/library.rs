use anyhow::{Result, bail};
use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

pub fn kind(path: &Path) -> Option<MediaKind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" | "png" | "heic" | "heif" | "tif" | "tiff" | "gif" | "webp" | "bmp"
        | "ico" | "avif" => Some(MediaKind::Image),
        "mov" | "mp4" | "m4v" => Some(MediaKind::Video),
        _ => None,
    }
}

pub fn collect(paths: &[PathBuf]) -> Result<(Vec<PathBuf>, usize)> {
    let Some(first) = paths.first() else {
        bail!("Choose an image, a video, or a folder.");
    };
    if paths.len() == 1 {
        if !first.exists() {
            bail!("This file is no longer available.");
        }
        if !first.is_dir() && kind(first).is_none() {
            bail!("This file type is not supported. Choose an image or an MP4, MOV, or M4V video.");
        }
    }
    let selected = if first.is_dir() {
        None
    } else {
        Some(first.clone())
    };
    let mut files = Vec::new();
    if paths.len() == 1 {
        let directory = if first.is_dir() {
            first.as_path()
        } else {
            first.parent().unwrap_or(Path::new("."))
        };
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_name().to_string_lossy().starts_with('.')
                && path.is_file()
                && kind(&path).is_some()
            {
                files.push(path);
            }
        }
    } else {
        for path in paths {
            if path.is_file() && kind(path).is_some() {
                files.push(path.clone());
            } else if path.is_dir() {
                files.extend(
                    collect(std::slice::from_ref(path))
                        .map(|(files, _)| files)
                        .unwrap_or_default(),
                );
            }
        }
    }
    files.sort_by(|a, b| {
        natural_cmp(
            &a.file_name().unwrap_or_default().to_string_lossy(),
            &b.file_name().unwrap_or_default().to_string_lossy(),
        )
        .then_with(|| a.cmp(b))
    });
    files.dedup();
    if files.is_empty() {
        bail!("No supported images or videos in this folder.");
    }
    let index = selected
        .and_then(|selected| files.iter().position(|p| p == &selected))
        .unwrap_or(0);
    Ok((files, index))
}

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a = a.to_lowercase();
    let b = b.to_lowercase();
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (start_a, start_b) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let mut da = &a[start_a..i];
            let mut db = &b[start_b..j];
            while da.first() == Some(&b'0') {
                da = &da[1..];
            }
            while db.first() == Some(&b'0') {
                db = &db[1..];
            }
            let cmp = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
            if cmp != Ordering::Equal {
                return cmp;
            }
        } else {
            let cmp = a[i].cmp(&b[j]);
            if cmp != Ordering::Equal {
                return cmp;
            }
            i += 1;
            j += 1;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural_order_handles_large_numbers_and_case() {
        let mut files = vec![
            "photo10.JPG",
            "photo2.jpg",
            "photo1.jpg",
            "photo99999999999999999999999.jpg",
        ];
        files.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            files,
            [
                "photo1.jpg",
                "photo2.jpg",
                "photo10.JPG",
                "photo99999999999999999999999.jpg"
            ]
        );
    }
    #[test]
    fn classifies_only_supported_media() {
        assert_eq!(kind(Path::new("PHOTO.HEIC")), Some(MediaKind::Image));
        assert_eq!(kind(Path::new("clip.MOV")), Some(MediaKind::Video));
        assert_eq!(kind(Path::new("notes.txt")), None);
    }

    #[test]
    fn folder_open_selects_requested_file_in_natural_order() {
        let dir = std::env::temp_dir().join(format!("petalook-folder-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "photo10.jpg",
            "photo2.jpg",
            "photo1.jpg",
            ".hidden.jpg",
            "notes.txt",
            "video.mp4",
        ] {
            fs::write(dir.join(name), []).unwrap();
        }
        fs::create_dir_all(dir.join("directory.png")).unwrap();
        let (files, index) = collect(&[dir.join("photo2.jpg")]).unwrap();
        assert_eq!(index, 1);
        assert_eq!(
            files
                .iter()
                .map(|p| p.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            ["photo1.jpg", "photo2.jpg", "photo10.jpg", "video.mp4"]
        );
        assert!(collect(&[dir.join("missing.jpg")]).is_err());
        assert!(collect(&[dir.join("notes.txt")]).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
