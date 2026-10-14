use crate::native;
use gpui::RenderImage;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, mpsc},
};

pub const PREVIEW_LIMIT: u32 = 3072;
pub const DETAIL_LIMIT: u32 = 8192;
const CACHE_BUDGET: usize = 320 * 1024 * 1024;

#[derive(Clone)]
pub struct Picture {
    pub image: Arc<RenderImage>,
    pub original: (u32, u32),
    pub limit: u32,
    bytes: usize,
}
pub struct Request {
    pub foreground: Option<(PathBuf, u32)>,
    pub prefetch: Vec<(PathBuf, u32)>,
    pub generation: u64,
}

struct Batch {
    items: VecDeque<(PathBuf, u32)>,
    generation: u64,
}

#[derive(Clone)]
pub struct Requests {
    foreground: mpsc::Sender<Batch>,
    prefetch: mpsc::Sender<Batch>,
}

impl Requests {
    pub fn send(&self, request: Request) {
        let Request {
            mut foreground,
            mut prefetch,
            generation,
        } = request;
        // A zoom-detail decode must not occupy the next image's preview worker.
        if foreground
            .as_ref()
            .is_some_and(|(_, limit)| *limit > PREVIEW_LIMIT)
        {
            prefetch.insert(0, foreground.take().unwrap());
        }
        let _ = self.foreground.send(Batch {
            items: foreground.into_iter().collect(),
            generation,
        });
        let _ = self.prefetch.send(Batch {
            items: prefetch.into(),
            generation,
        });
    }

    pub fn prime(&self, paths: &[PathBuf]) {
        if let Some(path) = paths.first()
            && crate::library::kind(path) == Some(crate::library::MediaKind::Image)
        {
            self.send(Request {
                foreground: Some((path.clone(), PREVIEW_LIMIT)),
                prefetch: vec![],
                generation: 1,
            });
        }
    }
}

pub struct Loaded {
    pub path: PathBuf,
    pub generation: u64,
    pub result: Result<Picture, String>,
}

pub struct Loader {
    pub requests: Requests,
    pub loaded: async_channel::Receiver<Loaded>,
}

pub fn start() -> Loader {
    start_with_decode(|path, limit| decode(path, limit).map_err(|e| e.to_string()))
}

fn start_with_decode(
    decode: impl Fn(&std::path::Path, u32) -> Result<Picture, String> + Send + Sync + 'static,
) -> Loader {
    let (results, loaded) = async_channel::bounded(2);
    let decode = Arc::new(decode);
    let worker = |prefetch: bool| {
        let (sender, requests) = mpsc::channel::<Batch>();
        let results = results.clone();
        let decode = decode.clone();
        std::thread::Builder::new()
            .name(
                if prefetch {
                    "image-prefetch"
                } else {
                    "image-decode"
                }
                .into(),
            )
            .spawn(move || {
                native::image_worker_priority(prefetch);
                let mut pending = None;
                while let Some(mut batch) = pending.take().or_else(|| requests.recv().ok()) {
                    while let Ok(newer) = requests.try_recv() {
                        batch = newer;
                    }
                    while let Some((path, limit)) = batch.items.pop_front() {
                        crate::startup::mark(if prefetch {
                            "prefetch_start"
                        } else {
                            "decode_start"
                        });
                        let result = decode(&path, limit);
                        crate::startup::mark(if prefetch {
                            "prefetch_end"
                        } else {
                            "decode_end"
                        });
                        while let Ok(newer) = requests.try_recv() {
                            pending = Some(newer);
                        }
                        let mut generation = batch.generation;
                        let wanted = if let Some(newer) = &mut pending {
                            let same_image =
                                newer.items.iter().any(|(p, l)| *p == path && *l <= limit);
                            newer.items.retain(|(p, l)| *p != path || *l > limit);
                            let wanted = same_image || newer.generation == generation;
                            generation = newer.generation;
                            wanted
                        } else {
                            true
                        };
                        if wanted
                            && results
                                .send_blocking(Loaded {
                                    path,
                                    generation,
                                    result,
                                })
                                .is_err()
                        {
                            return;
                        }
                        if pending.is_some() {
                            break;
                        }
                    }
                }
            })
            .expect("create image worker");
        sender
    };
    Loader {
        requests: Requests {
            foreground: worker(false),
            prefetch: worker(true),
        },
        loaded,
    }
}

fn decode(path: &std::path::Path, limit: u32) -> anyhow::Result<Picture> {
    let data = native::decode_image(path, limit)?;
    let bytes = data.pixels.len();
    let buffer = image::RgbaImage::from_raw(data.width, data.height, data.pixels)
        .ok_or_else(|| anyhow::anyhow!("Invalid image dimensions"))?;
    let image = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
        buffer
    )]));
    Ok(Picture {
        image,
        original: data.original,
        limit,
        bytes,
    })
}

#[derive(Default)]
pub struct Cache {
    entries: VecDeque<(PathBuf, Picture)>,
    bytes: usize,
}
impl Cache {
    pub fn has(&self, path: &std::path::Path, limit: u32) -> bool {
        self.entries.iter().any(|(p, picture)| {
            p == path
                && (picture.limit >= limit
                    || picture.original.0.max(picture.original.1) <= picture.limit)
        })
    }
    pub fn get(&mut self, path: &std::path::Path) -> Option<Picture> {
        let index = self.entries.iter().position(|(p, _)| p == path)?;
        let entry = self.entries.remove(index)?;
        let image = entry.1.clone();
        self.entries.push_back(entry);
        Some(image)
    }
    pub fn insert(&mut self, path: PathBuf, picture: Picture) -> Vec<Arc<RenderImage>> {
        let mut evicted = Vec::new();
        if let Some(index) = self.entries.iter().position(|(p, _)| p == &path) {
            let (_, old) = self.entries.remove(index).unwrap();
            self.bytes -= old.bytes;
            evicted.push(old.image);
        }
        self.bytes += picture.bytes;
        self.entries.push_back((path, picture));
        while self.bytes > CACHE_BUDGET || self.entries.len() > 5 {
            if let Some((_, old)) = self.entries.pop_front() {
                self.bytes -= old.bytes;
                evicted.push(old.image);
            }
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Mutex, time::Duration};

    fn receive(loader: &Loader) -> Loaded {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(result) = loader.loaded.try_recv() {
                return result;
            }
            assert!(std::time::Instant::now() < deadline, "decoder timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn foreground_does_not_wait_for_a_blocked_prefetch() {
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let wait_release = Mutex::new(wait_release);
        let loader = start_with_decode(move |path, _| {
            if path == std::path::Path::new("slow.png") {
                started.send(()).unwrap();
                wait_release.lock().unwrap().recv().unwrap();
            }
            Ok(picture())
        });
        loader.requests.send(Request {
            foreground: None,
            prefetch: vec![("slow.png".into(), PREVIEW_LIMIT)],
            generation: 1,
        });
        wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
        loader.requests.send(Request {
            foreground: Some(("selected.jpg".into(), PREVIEW_LIMIT)),
            prefetch: vec![],
            generation: 2,
        });
        let result = receive(&loader);
        release.send(()).unwrap();
        assert_eq!(result.path, PathBuf::from("selected.jpg"));
        assert_eq!(result.generation, 2);
    }

    #[test]
    fn navigation_does_not_wait_for_a_zoom_detail_decode() {
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let wait_release = Mutex::new(wait_release);
        let loader = start_with_decode(move |_, limit| {
            if limit == DETAIL_LIMIT {
                started.send(()).unwrap();
                wait_release.lock().unwrap().recv().unwrap();
            }
            Ok(picture())
        });
        loader.requests.send(Request {
            foreground: Some(("zoomed.png".into(), DETAIL_LIMIT)),
            prefetch: vec![],
            generation: 1,
        });
        wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
        loader.requests.send(Request {
            foreground: Some(("next.jpg".into(), PREVIEW_LIMIT)),
            prefetch: vec![],
            generation: 2,
        });
        let result = receive(&loader);
        release.send(()).unwrap();
        assert_eq!(result.path, PathBuf::from("next.jpg"));
        assert_eq!(result.generation, 2);
    }

    #[test]
    fn queued_requests_reuse_an_inflight_decode_and_skip_superseded_files() {
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let wait_release = Mutex::new(wait_release);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let loader = start_with_decode(move |path, _| {
            recorded.lock().unwrap().push(path.to_path_buf());
            if path == std::path::Path::new("selected.jpg") {
                started.send(()).unwrap();
                wait_release.lock().unwrap().recv().unwrap();
            }
            Ok(picture())
        });
        loader.requests.prime(&["selected.jpg".into()]);
        wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
        for (generation, path) in [(2, "superseded.jpg"), (3, "selected.jpg")] {
            loader.requests.send(Request {
                foreground: Some((path.into(), PREVIEW_LIMIT)),
                prefetch: vec![],
                generation,
            });
        }
        release.send(()).unwrap();
        let result = receive(&loader);
        assert_eq!(result.path, PathBuf::from("selected.jpg"));
        assert_eq!(result.generation, 3);
        assert_eq!(*calls.lock().unwrap(), vec![PathBuf::from("selected.jpg")]);
    }

    #[test]
    fn cancelled_decode_cannot_replace_the_new_selection() {
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let wait_release = Mutex::new(wait_release);
        let loader = start_with_decode(move |path, _| {
            if path == std::path::Path::new("old.jpg") {
                started.send(()).unwrap();
                wait_release.lock().unwrap().recv().unwrap();
            }
            Ok(picture())
        });
        loader.requests.prime(&["old.jpg".into()]);
        wait_started.recv_timeout(Duration::from_secs(2)).unwrap();
        loader.requests.send(Request {
            foreground: Some(("new.jpg".into(), PREVIEW_LIMIT)),
            prefetch: vec![],
            generation: 2,
        });
        release.send(()).unwrap();
        let result = receive(&loader);
        assert_eq!(result.path, PathBuf::from("new.jpg"));
        assert_eq!(result.generation, 2);
    }

    fn picture() -> Picture {
        Picture {
            image: Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
                image::RgbaImage::new(1, 1)
            )])),
            original: (1, 1),
            limit: PREVIEW_LIMIT,
            bytes: 4,
        }
    }
    #[test]
    fn cache_evicts_least_recently_used_and_recognizes_full_resolution() {
        let mut cache = Cache::default();
        for i in 0..5 {
            cache.insert(PathBuf::from(i.to_string()), picture());
        }
        cache.get(std::path::Path::new("0")).unwrap();
        assert_eq!(cache.insert(PathBuf::from("5"), picture()).len(), 1);
        assert!(cache.get(std::path::Path::new("1")).is_none());
        assert!(cache.has(std::path::Path::new("0"), DETAIL_LIMIT));
        assert_eq!(cache.bytes, 20);
    }
}
