use crate::common::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time,
};

impl FsDiff {
    pub fn apply(&self, root: &Path) -> Result<()> {
        use FsDiff::*;
        match self {
            Write(path, data) => fs::write(path_join(root, path), data)?,
            NewDir(path) => fs::create_dir(path_join(root, path))?,
            Del(path) => {
                let full_path = path_join(root, path);
                if fs::metadata(&full_path)?.is_dir() {
                    fs::remove_dir(full_path)?;
                } else {
                    fs::remove_file(full_path)?;
                }
            }
            Move(from, to) => fs::rename(path_join(root, from), path_join(root, to))?,
        };

        return Ok(());
    }

    pub fn register(&self, reg: &mut Reg) -> Result<()> {
        use FsDiff::*;
        use FsReg::*;
        return match self {
            Write(path, data) => {
                reg.insert(path.clone(), File(data.clone()));
                Ok(())
            }
            NewDir(path) => {
                reg.insert(path.clone(), Dir);
                Ok(())
            }
            Del(path) => match reg.remove(path) {
                Some(_) => Ok(()),
                None => Err(Error::Error("Register missing path".to_string())),
            },
            Move(from, to) => match reg.remove(from) {
                Some(file) => {
                    reg.insert(to.clone(), file);
                    Ok(())
                }
                None => Ok(()),
            },
        };
    }

    pub fn changes_register(&self, reg: &mut Reg) -> bool {
        use FsDiff::*;
        use FsReg::*;
        return match self {
            Write(path, data) => match reg.get(path) {
                Some(File(prev_data)) => data != prev_data,
                None => true,
                _ => false,
            },
            NewDir(path) => match reg.get(path) {
                Some(Dir) => false,
                _ => true,
            },
            Del(path) => match reg.get(path) {
                Some(_) => true,
                _ => false,
            },
            Move(from, to) => match (reg.get(from), reg.get(to)) {
                (Some(_), None) => true,
                (Some(from_file), Some(to_file)) => from_file != to_file,
                _ => false,
            },
        };
    }
}

pub fn load_fs(path: &Path) -> Result<Vec<FsDiff>> {
    fn helper(path: PathBuf, prefix: &Path, list: &mut Vec<FsDiff>) -> Result<()> {
        let stripped_path = strip_prefix(&path, prefix)?;
        if fs::metadata(&path)?.is_dir() {
            list.push(FsDiff::NewDir(stripped_path));
            for entry in fs::read_dir(&path)? {
                helper(entry?.path(), prefix, list)?;
            }
        } else {
            let data = fs::read(&path).unwrap_or(Vec::new());
            list.push(FsDiff::Write(stripped_path, data));
        }
        return Ok(());
    }

    let mut list = Vec::new();
    helper(path.to_path_buf(), path, &mut list)?;
    return Ok(list);
}

pub fn watch_fs(root: &Path, state: &SharedState, send: mpsc::Sender<Msg>) -> Result<()> {
    use notify::{watcher, DebouncedEvent::*, RecursiveMode, Watcher};

    let (notify_send, notify_receive) = mpsc::channel();

    let mut watcher = watcher(notify_send, time::Duration::from_millis(100))?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    {
        let diffs = load_fs(&root)?;
        let mut register = state.register.lock().unwrap();
        for diff in diffs {
            diff.register(&mut register)?;
            send.send(Msg {
                body: MsgBody::Remote(RemoteMsg::Diff(diff)),
                source: MsgSource::Inotify,
            })?;
        }
    }

    loop {
        let mut diffs = Vec::new();
        match notify_receive.recv()? {
            Create(path) if path.is_dir() => {
                diffs.push(FsDiff::NewDir(strip_prefix(&path, &root)?))
            }
            Create(path) | Write(path) => {
                let data = fs::read(&path).unwrap_or(Vec::new());
                diffs.push(FsDiff::Write(strip_prefix(&path, &root)?, data))
            }
            Remove(path) => diffs.push(FsDiff::Del(strip_prefix(&path, &root)?)),
            Rename(from, to) => diffs.push(FsDiff::Move(
                strip_prefix(&from, &root)?,
                strip_prefix(&to, &root)?,
            )),
            _ => (),
        }
        for diff in diffs {
            send.send(Msg {
                body: MsgBody::Remote(RemoteMsg::Diff(diff)),
                source: MsgSource::Inotify,
            })?;
        }
    }
}
