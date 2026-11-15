//! Background decode pipeline: a small worker pool fed by a priority queue,
//! with an LRU cache of decoded images and cancellation of stale work.
//!
//! The UI calls [`Loader::want`] with the paths it needs, current image first.
//! Anything no longer wanted is cancelled — queued jobs are dropped and
//! in-flight decodes are told to stop at their next cancellation check.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::cache::ByteLru;
use crate::cancel::CancelToken;
use crate::decode::{DecodeRequest, Decoders};
use crate::error::DecodeError;
use crate::pixels::DecodedImage;

pub const DEFAULT_CACHE_BYTES: usize = 768 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum LoadEvent {
    Loaded {
        path: PathBuf,
        image: Arc<DecodedImage>,
    },
    Failed {
        path: PathBuf,
        error: Arc<DecodeError>,
    },
}

impl LoadEvent {
    pub fn path(&self) -> &Path {
        match self {
            Self::Loaded { path, .. } | Self::Failed { path, .. } => path,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoaderConfig {
    pub threads: usize,
    pub cache_bytes: usize,
}

impl Default for LoaderConfig {
    fn default() -> Self {
        let cores = std::thread::available_parallelism().map_or(2, usize::from);
        Self {
            threads: cores.clamp(2, 4),
            cache_bytes: DEFAULT_CACHE_BYTES,
        }
    }
}

type Sink = Box<dyn Fn(LoadEvent) + Send + Sync>;

struct Job {
    path: PathBuf,
    priority: usize,
    seq: u64,
    cancel: CancelToken,
}

struct State {
    queue: Vec<Job>,
    inflight: HashMap<PathBuf, CancelToken>,
    cache: ByteLru<PathBuf, Arc<DecodedImage>>,
    seq: u64,
    target_size: Option<(u32, u32)>,
    shutdown: bool,
}

struct Shared {
    state: Mutex<State>,
    work: Condvar,
    decoders: Arc<Decoders>,
    sink: Sink,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A worker only panics outside the lock (decoder panics are caught), so
        // poisoning is not expected; recover the data rather than propagate.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub struct Loader {
    shared: Arc<Shared>,
}

impl Loader {
    pub fn new(
        decoders: Arc<Decoders>,
        config: LoaderConfig,
        sink: impl Fn(LoadEvent) + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                queue: Vec::new(),
                inflight: HashMap::new(),
                cache: ByteLru::new(config.cache_bytes),
                seq: 0,
                target_size: None,
                shutdown: false,
            }),
            work: Condvar::new(),
            decoders,
            sink: Box::new(sink),
        });
        for i in 0..config.threads.max(1) {
            let shared = Arc::clone(&shared);
            let spawned = std::thread::Builder::new()
                .name(format!("decode-{i}"))
                .spawn(move || worker(&shared));
            if let Err(err) = spawned {
                tracing::error!(%err, "failed to spawn decode thread");
            }
        }
        Self { shared }
    }

    /// Returns a cached image, marking it recently used.
    pub fn cached(&self, path: &Path) -> Option<Arc<DecodedImage>> {
        self.shared.lock().cache.get(path).cloned()
    }

    /// Seeds the cache, e.g. with an image decoded before the loader existed.
    pub fn insert(&self, path: PathBuf, image: Arc<DecodedImage>) {
        self.shared.lock().cache.insert(path, image);
    }

    /// Declares the images needed now, highest priority first. Everything
    /// else that is queued or decoding is cancelled.
    pub fn want(&self, paths: &[&Path]) {
        let mut st = self.shared.lock();
        let wanted = |p: &Path| paths.iter().position(|w| *w == p);

        st.queue.retain(|j| {
            let keep = wanted(&j.path).is_some();
            if !keep {
                j.cancel.cancel();
            }
            keep
        });
        for (path, token) in &st.inflight {
            if wanted(path).is_none() {
                token.cancel();
            }
        }

        for (priority, &path) in paths.iter().enumerate() {
            if st.cache.get(path).is_some() {
                continue;
            }
            if st.inflight.get(path).is_some_and(|t| !t.is_cancelled()) {
                continue;
            }
            if let Some(job) = st.queue.iter_mut().find(|j| j.path == path) {
                job.priority = priority;
                continue;
            }
            st.seq += 1;
            let seq = st.seq;
            st.queue.push(Job {
                path: path.to_owned(),
                priority,
                seq,
                cancel: CancelToken::new(),
            });
        }
        drop(st);
        self.shared.work.notify_all();
    }

    /// Forgets a cached image (file changed or was deleted).
    pub fn invalidate(&self, path: &Path) {
        let mut st = self.shared.lock();
        st.cache.remove(path);
        if let Some(t) = st.inflight.get(path) {
            t.cancel();
        }
    }

    pub fn clear_cache(&self) {
        self.shared.lock().cache.clear();
    }

    pub fn set_cache_budget(&self, bytes: usize) {
        self.shared.lock().cache.set_budget(bytes);
    }

    /// Display size hint passed to decoders (used by vector formats).
    pub fn set_target_size(&self, size: Option<(u32, u32)>) {
        self.shared.lock().target_size = size;
    }

    pub fn cache_usage(&self) -> (usize, usize) {
        let st = self.shared.lock();
        (st.cache.used(), st.cache.len())
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        let mut st = self.shared.lock();
        st.shutdown = true;
        for j in &st.queue {
            j.cancel.cancel();
        }
        for t in st.inflight.values() {
            t.cancel();
        }
        st.queue.clear();
        drop(st);
        self.shared.work.notify_all();
    }
}

fn worker(shared: &Shared) {
    loop {
        let (job, target_size) = {
            let mut st = shared.lock();
            loop {
                if st.shutdown {
                    return;
                }
                let best = st
                    .queue
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, j)| (j.priority, j.seq))
                    .map(|(i, _)| i);
                if let Some(i) = best {
                    let job = st.queue.swap_remove(i);
                    st.inflight.insert(job.path.clone(), job.cancel.clone());
                    break (job, st.target_size);
                }
                st = shared
                    .work
                    .wait(st)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };

        let req = DecodeRequest {
            cancel: job.cancel.clone(),
            target_size,
            ..DecodeRequest::default()
        };
        let span = tracing::debug_span!("decode", path = %job.path.display());
        let started = std::time::Instant::now();
        let result = span.in_scope(|| shared.decoders.decode_path(&job.path, &req));
        tracing::debug!(path = %job.path.display(), ms = started.elapsed().as_millis(), ok = result.is_ok(), "decoded");

        let event = {
            let mut st = shared.lock();
            if st
                .inflight
                .get(&job.path)
                .is_some_and(|t| t.same_as(&job.cancel))
            {
                st.inflight.remove(&job.path);
            }
            if job.cancel.is_cancelled() {
                continue;
            }
            match result {
                Ok(img) => {
                    let image = Arc::new(img);
                    st.cache.insert(job.path.clone(), Arc::clone(&image));
                    LoadEvent::Loaded {
                        path: job.path,
                        image,
                    }
                }
                Err(DecodeError::Cancelled) => continue,
                Err(e) => LoadEvent::Failed {
                    path: job.path,
                    error: Arc::new(e),
                },
            }
        };
        (shared.sink)(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::decode::Decoder;
    use crate::pixels::{Frame, PixelFormat};

    /// Decoder for fake files whose content is the delay in milliseconds.
    struct Slow;
    impl Decoder for Slow {
        fn name(&self) -> &'static str {
            "slow"
        }
        fn matches(&self, _: &[u8], ext: Option<&str>) -> bool {
            ext == Some("slow")
        }
        fn decode(&self, data: &[u8], req: &DecodeRequest) -> crate::Result<DecodedImage> {
            let ms: u64 = std::str::from_utf8(data)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            for _ in 0..ms {
                req.cancel.check()?;
                std::thread::sleep(Duration::from_millis(1));
            }
            let f =
                Frame::new(1, 1, PixelFormat::Gray8, vec![0]).ok_or(DecodeError::Unsupported)?;
            Ok(DecodedImage::still(f, "slow"))
        }
    }

    fn setup(files: &[(&str, u64)]) -> (PathBuf, Loader, mpsc::Receiver<LoadEvent>) {
        let dir = std::env::temp_dir().join(format!(
            "rustnior-loader-{}-{}",
            std::process::id(),
            files.first().map_or("", |f| f.0)
        ));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        for (name, ms) in files {
            std::fs::write(dir.join(name), ms.to_string()).expect("write");
        }
        let mut decoders = Decoders::empty();
        decoders.push(Box::new(Slow));
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let loader = Loader::new(
            Arc::new(decoders),
            LoaderConfig {
                threads: 1,
                cache_bytes: 1 << 20,
            },
            move |e| {
                let _ = tx.lock().map(|t| t.send(e));
            },
        );
        (dir, loader, rx)
    }

    fn recv(rx: &mpsc::Receiver<LoadEvent>) -> LoadEvent {
        rx.recv_timeout(Duration::from_secs(5)).expect("event")
    }

    #[test]
    fn loads_and_caches() {
        let (dir, loader, rx) = setup(&[("a1.slow", 0)]);
        let a = dir.join("a1.slow");
        loader.want(&[&a]);
        assert!(matches!(recv(&rx), LoadEvent::Loaded { ref path, .. } if *path == a));
        assert!(loader.cached(&a).is_some());
        // Already cached: no new decode.
        loader.want(&[&a]);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn stale_jobs_are_cancelled() {
        let (dir, loader, rx) = setup(&[("b1.slow", 2000), ("b2.slow", 0)]);
        let (slow, fast) = (dir.join("b1.slow"), dir.join("b2.slow"));
        loader.want(&[&slow]);
        std::thread::sleep(Duration::from_millis(50));
        // User navigated away: the in-flight slow decode must be abandoned.
        loader.want(&[&fast]);
        let ev = recv(&rx);
        assert_eq!(ev.path(), fast);
        assert!(loader.cached(&slow).is_none());
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn priority_order_is_respected() {
        let (dir, loader, rx) = setup(&[("c0.slow", 100), ("c1.slow", 0), ("c2.slow", 0)]);
        let p: Vec<PathBuf> = ["c0.slow", "c1.slow", "c2.slow"]
            .iter()
            .map(|n| dir.join(n))
            .collect();
        // Occupy the single worker, then queue two jobs, the later one first.
        loader.want(&[&p[0]]);
        std::thread::sleep(Duration::from_millis(20));
        loader.want(&[&p[0], &p[2], &p[1]]);
        let order: Vec<PathBuf> = (0..3).map(|_| recv(&rx).path().to_owned()).collect();
        assert_eq!(order, [p[0].clone(), p[2].clone(), p[1].clone()]);
    }

    #[test]
    fn rerequest_after_cancel_still_loads() {
        let (dir, loader, rx) = setup(&[("d1.slow", 300), ("d2.slow", 0)]);
        let (a, b) = (dir.join("d1.slow"), dir.join("d2.slow"));
        loader.want(&[&a]);
        std::thread::sleep(Duration::from_millis(20));
        loader.want(&[&b]);
        loader.want(&[&a]);
        let mut got: Vec<PathBuf> = Vec::new();
        while let Ok(e) = rx.recv_timeout(Duration::from_secs(3)) {
            got.push(e.path().to_owned());
            if got.contains(&a) {
                break;
            }
        }
        assert!(got.contains(&a));
    }

    #[test]
    fn failures_are_reported() {
        let (dir, loader, rx) = setup(&[("e1.slow", 0)]);
        let missing = dir.join("missing.slow");
        loader.want(&[&missing]);
        assert!(matches!(recv(&rx), LoadEvent::Failed { .. }));
    }
}
