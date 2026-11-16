//! Optional, pinned native assets. No inference library is loaded by this module.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
    pub format: String,
    pub strip: usize,
    #[serde(default)]
    pub filename: String,
    pub required: Vec<String>,
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub components: std::collections::BTreeMap<String, String>,
}

pub fn catalog() -> Vec<Asset> {
    serde_json::from_str(include_str!("../assets/catalog/downloads.json"))
        .expect("validated asset catalog")
}
pub fn root() -> PathBuf {
    crate::config::AppConfig::local_data_dir().join("assets")
}
pub fn directory(id: &str) -> PathBuf {
    root().join(id)
}
/// Saved assignments remain protected even when their selection mode is inactive.
pub fn assigned(id: &str, stt: &crate::config::AppConfig, reader: &reader_core::AppConfig) -> bool {
    let path = directory(id);
    let reader_assigned = [&reader.speech.selection, &reader.ocr.selection]
        .into_iter()
        .flat_map(|selection| {
            std::iter::once(&selection.single).chain(selection.languages.values())
        })
        .any(|binding| {
            binding.runtime.as_deref() == Some(id) || binding.model.as_deref() == Some(id)
        });
    stt.parakeet_runtime_dir_path() == path
        || stt
            .speech
            .language_runtimes
            .values()
            .any(|value| value.as_deref() == Some(id))
        || reader_assigned
}
pub fn available(asset: &Asset) -> bool {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    asset.platform == platform || asset.platform == "all"
}
pub fn installed_at(root: &Path, asset: &Asset) -> bool {
    let path = root.join(&asset.id);
    fs::read_to_string(path.join(".verified")).is_ok_and(|v| v == asset.sha256)
        && asset.required.iter().all(|file| path.join(file).exists())
}

#[derive(Clone, Default, Serialize)]
pub struct Progress {
    pub queued: Vec<String>,
    pub job_id: String,
    pub id: String,
    pub phase: String,
    pub downloaded: u64,
    pub total: u64,
    pub error: String,
}
#[derive(Default)]
struct Scheduler {
    jobs: std::collections::BTreeMap<String, Arc<Job>>,
    queue: std::collections::VecDeque<String>,
    active: usize,
}
#[derive(Default)]
struct Job {
    state: Mutex<Progress>,
    cancelled: AtomicBool,
}
pub type CompletionHook = Arc<dyn Fn(&str) -> bool + Send + Sync>;
#[derive(Default)]
struct Shared {
    scheduler: Mutex<Scheduler>,
    wake: Condvar,
    locked: Mutex<std::collections::HashSet<String>>,
    unlocked: Condvar,
    completion: Mutex<Option<CompletionHook>>,
    selection_revision: AtomicU64,
}
#[derive(Clone, Default)]
pub struct Manager {
    shared: Arc<Shared>,
}
struct AssetLocks {
    shared: Arc<Shared>,
    ids: Vec<String>,
}
impl Drop for AssetLocks {
    fn drop(&mut self) {
        let mut locked = self.shared.locked.lock().expect("asset locks");
        for id in &self.ids {
            locked.remove(id);
        }
        self.shared.unlocked.notify_all();
    }
}
impl Manager {
    pub fn set_completion_hook(&self, hook: CompletionHook) {
        *self.shared.completion.lock().expect("asset completion") = Some(hook);
    }
    pub fn selection_revision(&self) -> u64 {
        self.shared.selection_revision.load(Ordering::Acquire)
    }
    pub fn jobs(&self) -> Vec<Progress> {
        self.shared
            .scheduler
            .lock()
            .expect("asset scheduler")
            .jobs
            .values()
            .map(|job| job.state.lock().expect("asset progress").clone())
            .collect()
    }
    pub fn status(&self) -> Progress {
        let jobs = self.jobs();
        let mut status = jobs
            .iter()
            .find(|p| matches!(p.phase.as_str(), "downloading" | "verifying" | "extracting"))
            .or_else(|| jobs.iter().find(|p| p.phase == "queued"))
            .or_else(|| jobs.last())
            .cloned()
            .unwrap_or_default();
        status.queued = jobs
            .iter()
            .filter(|p| p.phase == "queued")
            .map(|p| p.job_id.clone())
            .collect();
        status
    }
    pub fn cancel(&self, id: &str) {
        let scheduler = self.shared.scheduler.lock().expect("asset scheduler");
        for (key, job) in &scheduler.jobs {
            if id.is_empty() || id == key {
                job.cancelled.store(true, Ordering::Release);
            }
        }
        self.shared.wake.notify_all();
        self.shared.unlocked.notify_all();
    }
    pub fn start(&self, id: &str) -> Result<()> {
        self.start_with(id, catalog(), root())
    }
    fn start_with(&self, id: &str, assets: Vec<Asset>, root: PathBuf) -> Result<()> {
        let asset = assets
            .iter()
            .find(|a| a.id == id)
            .context("Unknown asset")?
            .clone();
        anyhow::ensure!(
            available(&asset),
            "This download is not available for this platform"
        );
        let mut scheduler = self.shared.scheduler.lock().expect("asset scheduler");
        if scheduler.jobs.get(id).is_some_and(|job| {
            matches!(
                job.state.lock().expect("asset progress").phase.as_str(),
                "queued" | "downloading" | "verifying" | "extracting"
            )
        }) {
            return Ok(());
        }
        let job = Arc::new(Job {
            state: Mutex::new(Progress {
                id: id.into(),
                job_id: id.into(),
                phase: "queued".into(),
                ..Default::default()
            }),
            cancelled: AtomicBool::new(false),
        });
        scheduler.jobs.insert(id.into(), job.clone());
        scheduler.queue.push_back(id.into());
        drop(scheduler);
        let manager = self.clone();
        std::thread::spawn(move || manager.run_job(asset, assets, root, job));
        Ok(())
    }
    fn run_job(&self, asset: Asset, assets: Vec<Asset>, root: PathBuf, job: Arc<Job>) {
        let mut scheduler = self.shared.scheduler.lock().expect("asset scheduler");
        loop {
            if job.cancelled.load(Ordering::Acquire) {
                scheduler.queue.retain(|id| id != &asset.id);
                job.state.lock().expect("asset progress").phase = "cancelled".into();
                self.shared.wake.notify_all();
                return;
            }
            if scheduler.active < 2 && scheduler.queue.front() == Some(&asset.id) {
                scheduler.queue.pop_front();
                scheduler.active += 1;
                self.shared.wake.notify_all();
                break;
            }
            scheduler = self.shared.wake.wait(scheduler).expect("asset scheduler");
        }
        drop(scheduler);
        let mut ids = asset.dependencies.clone();
        ids.push(asset.id.clone());
        ids.sort();
        ids.dedup();
        let result = self
            .lock_assets(ids, Some(&job.cancelled))
            .and_then(|_locks| {
                for id in &asset.dependencies {
                    let dependency = assets
                        .iter()
                        .find(|a| a.id == *id)
                        .context("Unknown dependency")?;
                    job.install_one_at(dependency, &root)?;
                }
                job.install_one_at(&asset, &root)
            });
        if result.is_ok() {
            let hook = self
                .shared
                .completion
                .lock()
                .expect("asset completion")
                .clone();
            if hook.is_some_and(|hook| hook(&asset.id)) {
                self.shared
                    .selection_revision
                    .fetch_add(1, Ordering::AcqRel);
            }
        }
        let mut scheduler = self.shared.scheduler.lock().expect("asset scheduler");
        let mut state = job.state.lock().expect("asset progress");
        match result {
            Ok(()) => {
                state.id = asset.id.clone();
                state.phase = "ready".into();
            }
            Err(error) => {
                state.phase = if job.cancelled.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                state.error = format!("{error:#}");
            }
        }
        scheduler.active -= 1;
        self.shared.wake.notify_all();
    }
    fn lock_assets(&self, ids: Vec<String>, cancelled: Option<&AtomicBool>) -> Result<AssetLocks> {
        let mut locked = self.shared.locked.lock().expect("asset locks");
        loop {
            if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
                bail!("Download cancelled");
            }
            if ids.iter().all(|id| !locked.contains(id)) {
                locked.extend(ids.iter().cloned());
                return Ok(AssetLocks {
                    shared: self.shared.clone(),
                    ids,
                });
            }
            anyhow::ensure!(cancelled.is_some(), "Wait for the download to finish");
            let (next, _) = self
                .shared
                .unlocked
                .wait_timeout(locked, Duration::from_millis(50))
                .expect("asset locks");
            locked = next;
        }
    }
    pub fn remove(&self, id: &str) -> Result<()> {
        let assets = catalog();
        let asset = assets
            .iter()
            .find(|a| a.id == id)
            .context("Unknown asset")?;
        let _locks = self.lock_assets(vec![id.into()], None)?;
        for other in &assets {
            anyhow::ensure!(
                !other.dependencies.contains(&asset.id) || !installed_at(&root(), other),
                "Remove {} first",
                other.label
            );
        }
        if directory(id).exists() {
            fs::remove_dir_all(directory(id))?;
        }
        Ok(())
    }
}
impl Job {
    fn install_one_at(&self, asset: &Asset, root: &Path) -> Result<()> {
        self.check_cancelled()?;
        fs::create_dir_all(root)?;
        if installed_at(root, asset) {
            return Ok(());
        }
        let staging = tempfile_dir(root, &asset.id)?;
        let result = (|| {
            let archive = staging.join("download");
            {
                let mut state = self.state.lock().expect("asset progress");
                state.id = asset.id.clone();
                state.phase = "downloading".into();
                state.downloaded = 0;
                state.total = asset.bytes;
            }
            let mut output = File::create(&archive)?;
            let mut hash = Sha256::new();
            let count = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(async {
                    let client = reqwest::Client::builder()
                        .connect_timeout(Duration::from_secs(20))
                        .read_timeout(Duration::from_secs(30))
                        .build()?;
                    let mut response = tokio::select! {
                        result = client.get(&asset.url).send() => result?.error_for_status()?,
                        _ = wait_cancelled(&self.cancelled) => bail!("Download cancelled"),
                    };
                    let mut count = 0u64;
                    loop {
                        self.check_cancelled()?;
                        let chunk = tokio::select! {
                            result = response.chunk() => result?,
                            _ = wait_cancelled(&self.cancelled) => bail!("Download cancelled"),
                        };
                        let Some(chunk) = chunk else {
                            break;
                        };
                        count += chunk.len() as u64;
                        anyhow::ensure!(count <= asset.bytes, "Download exceeded its pinned size");
                        hash.update(&chunk);
                        output.write_all(&chunk)?;
                        self.state.lock().expect("asset progress").downloaded = count;
                    }
                    Ok::<u64, anyhow::Error>(count)
                })?;
            output.sync_all()?;
            self.state.lock().expect("asset progress").phase = "verifying".into();
            anyhow::ensure!(
                count == asset.bytes && format!("{:x}", hash.finalize()) == asset.sha256,
                "Download checksum mismatch"
            );
            self.check_cancelled()?;
            self.state.lock().expect("asset progress").phase = "extracting".into();
            let destination = staging.join("content");
            fs::create_dir(&destination)?;
            unpack(asset, &archive, &destination, &self.cancelled)?;
            for (target, source) in &asset.components {
                let source = relative(Path::new(source), 0)?;
                let dependency = source
                    .components()
                    .next()
                    .context("Empty component source")?
                    .as_os_str()
                    .to_string_lossy();
                anyhow::ensure!(
                    asset.dependencies.iter().any(|id| id == &dependency),
                    "Component must belong to a declared dependency"
                );
                let target = destination.join(relative(Path::new(target), 0)?);
                anyhow::ensure!(!target.exists(), "Duplicate component target");
                fs::hard_link(root.join(source), target).context("Assembling model components")?;
            }
            for file in &asset.required {
                anyhow::ensure!(destination.join(file).exists(), "Asset is missing {file}");
            }
            self.check_cancelled()?;
            fs::write(destination.join(".verified"), &asset.sha256)?;
            let target = root.join(&asset.id);
            let previous = staging.join("previous");
            if target.exists() {
                fs::rename(&target, &previous)?;
            }
            if let Err(error) = fs::rename(&destination, &target) {
                if previous.exists() {
                    let _ = fs::rename(&previous, &target);
                }
                return Err(error).context("Activating verified asset");
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }
    fn check_cancelled(&self) -> Result<()> {
        anyhow::ensure!(
            !self.cancelled.load(Ordering::Acquire),
            "Download cancelled"
        );
        Ok(())
    }
}
async fn wait_cancelled(cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn tempfile_dir(root: &Path, id: &str) -> Result<PathBuf> {
    let path = root.join(format!(".{}-{}", id, rand::random::<u64>()));
    fs::create_dir(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}
fn relative(path: &Path, strip: usize) -> Result<PathBuf> {
    anyhow::ensure!(
        path.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
        "Unsafe archive path"
    );
    Ok(path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .skip(strip)
        .collect())
}
fn unpack(asset: &Asset, archive: &Path, destination: &Path, cancelled: &AtomicBool) -> Result<()> {
    if asset.format == "file" {
        let name = relative(Path::new(&asset.filename), 0)?;
        anyhow::ensure!(!name.as_os_str().is_empty(), "Missing asset filename");
        fs::copy(archive, destination.join(name))?;
        return Ok(());
    }
    let mut bytes = 0u64;
    if asset.format == "zip" {
        let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
        anyhow::ensure!(zip.len() <= 100_000, "Archive has too many entries");
        for index in 0..zip.len() {
            anyhow::ensure!(!cancelled.load(Ordering::Acquire), "Download cancelled");
            let mut entry = zip.by_index(index)?;
            let name = relative(
                &entry.enclosed_name().context("Unsafe zip path")?,
                asset.strip,
            )?;
            if name.as_os_str().is_empty() {
                continue;
            }
            anyhow::ensure!(
                entry
                    .unix_mode()
                    .is_none_or(|mode| mode & 0o170000 != 0o120000),
                "Zip symlinks are not supported"
            );
            bytes += entry.size();
            anyhow::ensure!(bytes <= 6 * 1024 * 1024 * 1024, "Archive is too large");
            let target = destination.join(name);
            if entry.is_dir() {
                fs::create_dir_all(target)?;
            } else {
                fs::create_dir_all(target.parent().context("Missing parent")?)?;
                copy_cancellable(&mut entry, &mut File::create(target)?, cancelled)?;
            }
        }
        return Ok(());
    }
    let file = File::open(archive)?;
    let reader: Box<dyn Read> = match asset.format.as_str() {
        "tar_gz" => Box::new(flate2::read::GzDecoder::new(file)),
        "tar_bz2" => Box::new(bzip2::read::BzDecoder::new(file)),
        _ => bail!("Unsupported archive format"),
    };
    let mut tar = tar::Archive::new(reader);
    let mut links = Vec::new();
    for (index, entry) in tar.entries()?.enumerate() {
        anyhow::ensure!(index < 100_000, "Archive has too many entries");
        anyhow::ensure!(!cancelled.load(Ordering::Acquire), "Download cancelled");
        let mut entry = entry?;
        let name = relative(&entry.path()?, asset.strip)?;
        if name.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(&name);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
        } else if kind.is_file() {
            bytes += entry.size();
            anyhow::ensure!(bytes <= 6 * 1024 * 1024 * 1024, "Archive is too large");
            fs::create_dir_all(target.parent().context("Missing parent")?)?;
            copy_cancellable(&mut entry, &mut File::create(&target)?, cancelled)?;
        } else if kind.is_symlink() || kind.is_hard_link() {
            let link = entry.link_name()?.context("Missing link target")?;
            let source = if kind.is_hard_link() {
                relative(&link, asset.strip)?
            } else {
                name.parent()
                    .unwrap_or(Path::new(""))
                    .join(relative(&link, 0)?)
            };
            links.push((target, destination.join(source)));
        } else {
            bail!("Unsupported archive entry type");
        }
    }
    // Materialize validated library aliases as ordinary files, never links outside the asset.
    for _ in 0..8 {
        let mut pending = Vec::new();
        for (target, source) in links {
            if source.is_file() {
                fs::create_dir_all(target.parent().context("Missing parent")?)?;
                if fs::hard_link(&source, &target).is_err() {
                    bytes += fs::metadata(&source)?.len();
                    anyhow::ensure!(bytes <= 6 * 1024 * 1024 * 1024, "Archive is too large");
                    copy_cancellable(
                        &mut File::open(source)?,
                        &mut File::create(target)?,
                        cancelled,
                    )?;
                }
            } else {
                pending.push((target, source));
            }
        }
        if pending.is_empty() {
            return Ok(());
        }
        links = pending;
    }
    anyhow::ensure!(links.is_empty(), "Unresolved archive links");
    Ok(())
}

fn copy_cancellable(
    input: &mut impl Read,
    output: &mut impl Write,
    cancelled: &AtomicBool,
) -> Result<()> {
    let mut buffer = vec![0; 128 * 1024];
    loop {
        anyhow::ensure!(!cancelled.load(Ordering::Acquire), "Download cancelled");
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unused_reader_path_defaults_do_not_lock_downloaded_assets() {
        let stt = crate::config::AppConfig::default();
        let reader = crate::reader::defaults();
        for id in ["kokoro", "paddle-ar-en", "sherpa-cpu"] {
            assert!(!assigned(id, &stt, &reader), "{id}");
        }
    }
    #[test]
    fn saved_assignments_protect_assets_in_inactive_modes() {
        let mut stt = crate::config::AppConfig::default();
        let mut reader = reader_core::AppConfig::default();
        stt.speech
            .language_runtimes
            .insert("ar".into(), Some("parakeet-cuda".into()));
        stt.speech.selection_mode = crate::config::ModelSelectionMode::SingleModel;
        reader.speech.selection.languages.insert(
            "en".into(),
            reader_core::routing::Binding {
                runtime: Some("sherpa-cpu".into()),
                model: Some("kokoro".into()),
                ..Default::default()
            },
        );
        reader.ocr.selection.single.model = Some("ocr-eng".into());
        for id in ["parakeet-cuda", "sherpa-cpu", "kokoro", "ocr-eng"] {
            assert!(assigned(id, &stt, &reader), "{id}");
        }
        assert!(!assigned("unassigned", &stt, &reader));
    }
    #[test]
    fn catalog_is_pinned_and_closed() {
        let assets = catalog();
        let mut ids = std::collections::HashSet::new();
        for asset in &assets {
            assert!(ids.insert(&asset.id));
            assert_eq!(asset.sha256.len(), 64);
            assert!(asset.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
            assert!(asset.url.starts_with("https://"));
            assert!(asset.bytes > 0);
            assert_eq!(
                relative(Path::new(&asset.id), 0)
                    .unwrap()
                    .components()
                    .count(),
                1
            );
            for id in &asset.dependencies {
                assert!(assets
                    .iter()
                    .any(|a| a.id == *id && a.dependencies.is_empty()));
            }
        }
    }
    #[test]
    fn archive_paths_cannot_escape_or_be_absolute() {
        for name in ["../escape", "/tmp/escape", "folder/../../escape"] {
            assert!(relative(Path::new(name), 0).is_err());
        }
        assert_eq!(
            relative(Path::new("root/lib/test.so"), 1).unwrap(),
            PathBuf::from("lib/test.so")
        );
    }
    #[test]
    fn incomplete_assets_are_not_ready() {
        let root = tempfile::tempdir().unwrap();
        let asset = catalog().remove(0);
        let dir = root.path().join(&asset.id);
        fs::create_dir(&dir).unwrap();
        assert!(!installed_at(root.path(), &asset));
        fs::write(dir.join(".verified"), &asset.sha256).unwrap();
        assert!(!installed_at(root.path(), &asset));
        fs::write(dir.join(&asset.required[0]), b"fixture").unwrap();
        assert!(installed_at(root.path(), &asset));
    }
}

#[cfg(test)]
mod delivery_tests {
    use super::*;
    fn fixture(bytes: Vec<u8>) -> (Asset, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/fixture", listener.local_addr().unwrap());
        let mut asset = catalog().remove(0);
        asset.id = "fixture".into();
        asset.format = "file".into();
        asset.filename = "payload".into();
        asset.required = vec!["payload".into()];
        asset.bytes = bytes.len() as u64;
        asset.sha256 = format!("{:x}", Sha256::digest(&bytes));
        asset.url = url;
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            stream.write_all(&bytes).unwrap();
        });
        (asset, thread)
    }
    fn stalled_fixture(
        id: &str,
    ) -> (
        Asset,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Sender<()>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut asset = catalog().remove(0);
        asset.id = id.into();
        asset.platform = "all".into();
        asset.url = format!("http://{}/asset", listener.local_addr().unwrap());
        asset.format = "file".into();
        asset.filename = "payload".into();
        asset.required = vec!["payload".into()];
        asset.dependencies.clear();
        asset.components.clear();
        asset.bytes = 7;
        asset.sha256 = format!("{:x}", Sha256::digest(b"fixture"));
        let (hit_tx, hit_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\n");
            let _ = hit_tx.send(());
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
            let _ = stream.write_all(b"fixture");
        });
        (asset, hit_rx, release_tx, server)
    }

    fn wait_phase(manager: &Manager, id: &str, phase: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if manager
                .jobs()
                .iter()
                .any(|job| job.job_id == id && job.phase == phase)
            {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{id} did not reach {phase}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn independent_jobs_cancel_and_queue_separately() {
        let root = tempfile::tempdir().unwrap();
        let (a, hit_a, release_a, server_a) = stalled_fixture("a");
        let (b, hit_b, release_b, server_b) = stalled_fixture("b");
        let mut c = a.clone();
        c.id = "c".into();
        let assets = vec![a, b, c];
        let manager = Manager::default();
        manager
            .start_with("a", assets.clone(), root.path().into())
            .unwrap();
        manager
            .start_with("b", assets.clone(), root.path().into())
            .unwrap();
        hit_a.recv_timeout(Duration::from_secs(3)).unwrap();
        hit_b.recv_timeout(Duration::from_secs(3)).unwrap();
        manager.start_with("c", assets, root.path().into()).unwrap();
        wait_phase(&manager, "c", "queued");
        assert!(manager.lock_assets(vec!["a".into()], None).is_err());
        drop(manager.lock_assets(vec!["unrelated".into()], None).unwrap());
        manager.cancel("c");
        manager.cancel("a");
        wait_phase(&manager, "c", "cancelled");
        wait_phase(&manager, "a", "cancelled");
        release_b.send(()).unwrap();
        wait_phase(&manager, "b", "ready");
        let _ = release_a.send(());
        server_a.join().unwrap();
        server_b.join().unwrap();
        assert!(root.path().join("b/payload").exists());
        assert!(!root.path().join("a").exists());
    }

    #[test]
    fn concurrent_jobs_reuse_a_shared_verified_dependency() {
        let root = tempfile::tempdir().unwrap();
        let (dependency, hit, release, server) = stalled_fixture("shared");
        let (mut a, server_a) = fixture(b"asset a".to_vec());
        let (mut b, server_b) = fixture(b"asset b".to_vec());
        for asset in [&mut a, &mut b] {
            asset.platform = "all".into();
            asset.dependencies = vec!["shared".into()];
            asset.components.clear();
        }
        a.id = "a".into();
        b.id = "b".into();
        let assets = vec![dependency, a, b];
        let manager = Manager::default();
        manager
            .start_with("a", assets.clone(), root.path().into())
            .unwrap();
        hit.recv_timeout(Duration::from_secs(3)).unwrap();
        manager.start_with("b", assets, root.path().into()).unwrap();
        release.send(()).unwrap();
        wait_phase(&manager, "a", "ready");
        wait_phase(&manager, "b", "ready");
        server.join().unwrap();
        server_a.join().unwrap();
        server_b.join().unwrap();
        assert!(root.path().join("shared/payload").exists());
    }

    #[test]
    fn pinned_nvidia_wheels_install_their_actual_release_layout() {
        use std::io::Cursor;
        let mut failures = Vec::new();
        for (id, files) in [
            (
                "cuda-cublas",
                vec![
                    "nvidia/cu13/lib/libcublas.so.13",
                    "nvidia/cu13/lib/libcublasLt.so.13",
                ],
            ),
            (
                "cuda-nvrtc",
                vec![
                    "nvidia/cu13/lib/libnvrtc.so.13",
                    "nvidia/cu13/lib/libnvrtc-builtins.so.13.4",
                ],
            ),
        ] {
            // Paths come from the central directories of the pinned NVIDIA wheels.
            let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for name in &files {
                archive
                    .start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                archive.write_all(b"public library fixture").unwrap();
            }
            let bytes = archive.finish().unwrap().into_inner();
            let (mut asset, server) = fixture(bytes);
            asset.id = id.into();
            asset.format = "zip".into();
            asset.strip = 0;
            asset.required = catalog()
                .into_iter()
                .find(|entry| entry.id == id)
                .unwrap()
                .required;
            let root = tempfile::tempdir().unwrap();
            let result = Job::default().install_one_at(&asset, root.path());
            server.join().unwrap();
            if let Err(error) = result {
                failures.push(format!("{id}: {error:#}"));
            } else {
                assert!(installed_at(root.path(), &asset));
                for name in &files {
                    assert!(root.path().join(id).join(name).is_file());
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn verified_download_activates_only_after_complete_transfer() {
        let root = tempfile::tempdir().unwrap();
        let (asset, server) = fixture(b"native asset fixture".to_vec());
        Job::default().install_one_at(&asset, root.path()).unwrap();
        server.join().unwrap();
        assert!(installed_at(root.path(), &asset));
        assert_eq!(
            fs::read(root.path().join("fixture/payload")).unwrap(),
            b"native asset fixture"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn corrupt_download_never_becomes_installed() {
        let root = tempfile::tempdir().unwrap();
        let (mut asset, server) = fixture(b"corrupt".to_vec());
        asset.sha256 = "0".repeat(64);
        let error = Job::default()
            .install_one_at(&asset, root.path())
            .unwrap_err();
        server.join().unwrap();
        assert!(error.to_string().contains("checksum"));
        assert!(!installed_at(root.path(), &asset));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[test]
    fn cancellation_interrupts_a_stalled_network_read() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut asset = catalog().remove(0);
        asset.id = "stalled".into();
        asset.url = format!("http://{}/stalled", listener.local_addr().unwrap());
        let (release, wait) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\n");
            let _ = wait.recv_timeout(Duration::from_secs(3));
        });
        let manager = Arc::new(Job::default());
        let cancellation = manager.clone();
        let cancel = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            cancellation.cancelled.store(true, Ordering::Release);
        });
        let root = tempfile::tempdir().unwrap();
        let start = std::time::Instant::now();
        assert!(manager
            .install_one_at(&asset, root.path())
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        let _ = release.send(());
        server.join().unwrap();
        cancel.join().unwrap();
    }
    #[test]
    fn cancelled_extraction_does_not_publish_content() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("test.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            File::create(&archive).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "root/lib.so", &b"hello"[..])
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        let mut asset = catalog().remove(0);
        asset.strip = 1;
        assert!(
            unpack(&asset, &archive, root.path(), &AtomicBool::new(true))
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(!root.path().join("lib.so").exists());
        unpack(&asset, &archive, root.path(), &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read(root.path().join("lib.so")).unwrap(), b"hello");
    }
}
