use notify::{DebouncedEvent, Watcher};
use std::{
    env, fs, io, net,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error")]
    IoError(#[from] io::Error),
    #[error("Strip prefix error")]
    StripPrefixError(#[from] std::path::StripPrefixError),
    #[error("Notify error")]
    NotifyError(#[from] notify::Error),
    #[error("Receive error")]
    RecvError(#[from] mpsc::RecvError),
    #[error("Send error")]
    SendError(#[from] mpsc::SendError<FsDiff>),
    #[error("Error: {0}")]
    Error(String),
}

type Result<T> = std::result::Result<T, Error>;

fn strip_prefix(path: &PathBuf, prefix: &Path) -> Result<PathBuf> {
    return Ok(PathBuf::from(path.strip_prefix(prefix)?));
}

fn path_join(prefix: &Path, path: &Path) -> PathBuf {
    return [prefix, path].iter().collect();
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub enum FsDiff {
    Write(PathBuf, Vec<u8>),
    NewDir(PathBuf),
    Del(PathBuf),
    Move(PathBuf, PathBuf),
}

impl FsDiff {
    fn apply(&self, root: &Path) -> Result<()> {
        match self {
            FsDiff::Write(path, data) => fs::write(path_join(root, path), data)?,
            FsDiff::NewDir(path) => fs::create_dir(path_join(root, path))?,
            FsDiff::Del(path) => {
                let full_path = path_join(root, path);
                if fs::metadata(&full_path)?.is_dir() {
                    fs::remove_dir(full_path)?;
                } else {
                    fs::remove_file(full_path)?;
                }
            }
            FsDiff::Move(from, to) => fs::rename(path_join(root, from), path_join(root, to))?,
        };

        return Ok(());
    }
}

fn initial_diffs(path: &Path) -> Result<Vec<FsDiff>> {
    fn helper(path: PathBuf, prefix: &Path, list: &mut Vec<FsDiff>) -> Result<()> {
        if fs::metadata(&path)?.is_dir() {
            list.push(FsDiff::NewDir(strip_prefix(&path, prefix)?));
            for entry in fs::read_dir(&path)? {
                helper(entry?.path(), prefix, list)?;
            }
        } else {
            let data = fs::read(&path).unwrap_or(Vec::new());
            list.push(FsDiff::Write(path, data));
        }
        return Ok(());
    }

    let mut list = Vec::new();
    helper(path.to_path_buf(), path, &mut list)?;
    return Ok(list);
}

fn watch_fs(root: &Path, send: mpsc::Sender<FsDiff>) -> Result<()> {
    let (notify_send, notify_receive) = mpsc::channel();

    let mut watcher = notify::watcher(notify_send, Duration::from_millis(100))?;
    watcher.watch(&root, notify::RecursiveMode::Recursive)?;

    for diff in initial_diffs(&root)? {
        send.send(diff)?;
    }

    loop {
        let mut diffs = Vec::new();
        match notify_receive.recv()? {
            DebouncedEvent::Create(path) if path.is_dir() => {
                diffs.push(FsDiff::NewDir(strip_prefix(&path, &root)?))
            }
            DebouncedEvent::Create(path) | DebouncedEvent::Write(path) => {
                let data = fs::read(&path).unwrap_or(Vec::new());
                diffs.push(FsDiff::Write(strip_prefix(&path, &root)?, data))
            }
            DebouncedEvent::Remove(path) => diffs.push(FsDiff::Del(strip_prefix(&path, &root)?)),
            DebouncedEvent::Rename(from, to) => diffs.push(FsDiff::Move(
                strip_prefix(&from, &root)?,
                strip_prefix(&to, &root)?,
            )),
            _ => (),
        }
        for diff in diffs {
            send.send(diff)?;
        }
    }
}

fn main() -> Result<()> {
    let root = env::current_dir()?;

    let (diff_send, diff_receive) = mpsc::channel();
    let fs_watcher = {
        let root_clone = root.clone();
        thread::spawn(move || watch_fs(&root_clone, diff_send).expect("whoops"));
    };

    loop {
        match diff_receive.recv() {
            Ok(diff) => println!("{:?}", diff),
            Err(err) => (), //println!("{:?}", err),
        }
    }
}
