use crate::collabignore;
use crate::common::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    sync::Arc,
    thread, time,
};

impl FsDiff {
    /// Apply this FsDiff to the local filesystem
    #[context("unable to apply fs diff: {:?}, root: {}", self, root.display())]
    pub fn apply(&self, root: &Path) -> Result<()> {
        use FsDiff::*;
        match self {
            Write(path, data) => fs::write(path_join(root, path), &data[..])?,
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
            Chmod(path, perm) => {
                perm.set(&path_join(root, path))?;
            }
        };

        return Ok(());
    }

    /// Register this FsDiff to the file registry
    #[context("unable to register fs diff: {:?}", self)]
    pub fn register(&self, reg: &mut Reg) -> Result<()> {
        use FsDiff::*;
        use FsReg::*;
        return match self {
            Write(path, data) => {
                let perm = match reg.get(path) {
                    Some(FsReg::File(_, Some(perm))) => Some(perm.clone()),
                    _ => None,
                };
                reg.insert(path.clone(), File(hash_file(data), perm));
                Ok(())
            }
            NewDir(path) => {
                reg.insert(path.clone(), Dir);
                Ok(())
            }
            Del(path) => match reg.remove(path) {
                Some(_) => Ok(()),
                None => Err(CollabError::Error("Register missing path".to_string()).into()),
            },
            Move(from, to) => match reg.remove(from) {
                Some(file) => {
                    reg.insert(to.clone(), file);
                    Ok(())
                }
                None => Ok(()),
            },
            Chmod(path, perm) => {
                let data = match reg.get(path) {
                    Some(FsReg::File(data, _)) => data.clone(),
                    _ => {
                        return Err(CollabError::Error(
                            format!("Unable to apply chmod to path {}", path).to_string(),
                        )
                        .into());
                    }
                };
                reg.insert(path.clone(), File(data, Some(perm.clone())));
                Ok(())
            }
        };
    }

    /// Returns true if this FsDiff would would update the file registry
    pub fn changes_register(&self, reg: &mut Reg) -> bool {
        use FsDiff::*;
        use FsReg::*;
        return match self {
            Write(path, data) => match reg.get(path) {
                Some(File(prev_data, _)) => &hash_file(data) != prev_data,
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
            Chmod(path, perm) => {
                return match reg.get(path) {
                    Some(File(_, old_perm)) => Some(perm) != old_perm.as_ref(),
                    _ => false,
                }
            }
        };
    }
}

#[context("unable to load fs, root: {}", root.display())]
pub fn load_fs(root: &Path, state: &SharedState) -> Result<Vec<FsDiff>> {
    let mut list = Vec::new();
    for entry in collabignore::build_walker(root) {
        let entry = entry?;
        let path = entry.path();
        let stripped_path = strip_prefix(path, root)?;
        if entry.metadata()?.is_dir() {
            list.push(FsDiff::NewDir(stripped_path));
        } else {
            let data = fs::read(&path).unwrap_or(Vec::new());
            match FilePerm::get(path) {
                Ok(perms) => {
                    list.push(FsDiff::Write(stripped_path.clone(), Arc::new(data)));
                    list.push(FsDiff::Chmod(stripped_path, perms));
                    if collabignore::is_ignore_file(path) {
                        state.ignore.lock().unwrap().ignore_file_modified(path)?;
                    }
                }
                Err(_) => (), // file may have been deleted or moved
            }
        }
    }
    return Ok(list);
}

pub fn load_fs_and_send_parallel(
    root: &Path,
    state: &SharedState,
    send: &mpsc::Sender<Msg>,
    do_register: bool,
) {
    let (root, state, send) = (PathBuf::from(root), state.clone(), send.clone());
    thread::spawn(move || -> Result<()> {
        let diffs = load_fs(&root, &state)?;
        if do_register {
            let mut register = state.register.lock().unwrap();
            for diff in diffs {
                diff.register(&mut register)?;
                send.send(Msg {
                    body: MsgBody::Remote(RemoteMsg::FsDiff(diff)),
                    source: MsgSource::Inotify,
                })
                .map_err(|err| {
                    CollabError::Error(format!("Error sending registered fs diff: {}", err))
                })?;
            }
        } else {
            for diff in diffs {
                send.send(Msg {
                    body: MsgBody::Remote(RemoteMsg::FsDiff(diff)),
                    source: MsgSource::Inotify,
                })
                .map_err(|err| CollabError::Error(format!("Error sending fs diff: {}", err)))?;
            }
        }
        return Ok(());
    });
}

#[context("unable to watch fs, root: {}", root.display())]
pub fn watch_fs(root: &Path, state: &SharedState, send: mpsc::Sender<Msg>) -> Result<()> {
    use notify::{watcher, DebouncedEvent::*, RecursiveMode, Watcher};

    let (notify_send, notify_receive) = mpsc::channel();

    let mut watcher = watcher(notify_send, time::Duration::from_millis(100))?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    // load initial FS
    load_fs_and_send_parallel(root, state, &send, true);

    // There is a lot of special handling for ignored files. In particular,
    // if a .gitignore/.ignore file is changed, moved, or removed, then we
    // need to reload the FS because there might be files that were formerly
    // ignored that we need to pick up. Note that load_fs_and_send_parallel
    // also acquires the ignore lock, so if we try to run it in the same
    // thread then we will get deadlock. The current implementation is fine
    // because it spawns a separate thread.

    loop {
        let mut diffs = Vec::new();
        match notify_receive.recv()? {
            Create(path) if path.is_dir() => {
                if !state.ignore.lock().unwrap().is_ignored(&path) {
                    diffs.push(FsDiff::NewDir(strip_prefix(&path, &root)?))
                }
            }
            Create(path) | Write(path) => {
                let mut ignore = state.ignore.lock().unwrap();
                if collabignore::is_ignore_file(&path) {
                    ignore.ignore_file_modified(&path)?;
                    load_fs_and_send_parallel(root, state, &send, false);
                }
                if !ignore.is_ignored(&path) {
                    let data = fs::read(&path).unwrap_or(Vec::new());
                    let relative_path = strip_prefix(&path, &root)?;
                    match FilePerm::get(&path) {
                        Ok(perms) => {
                            diffs.push(FsDiff::Write(relative_path.clone(), Arc::new(data)));
                            diffs.push(FsDiff::Chmod(relative_path, perms));
                        }
                        Err(_) => (), // file may have been deleted or moved
                    }
                }
            }
            Remove(path) => {
                let mut ignore = state.ignore.lock().unwrap();
                if collabignore::is_ignore_file(&path) {
                    ignore.ignore_file_removed(&path)?;
                    load_fs_and_send_parallel(root, state, &send, false);
                }
                if !ignore.is_ignored(&path) {
                    diffs.push(FsDiff::Del(strip_prefix(&path, &root)?))
                }
            }
            Rename(from, to) => {
                let mut ignore = state.ignore.lock().unwrap();

                let mut reload = false;
                if collabignore::is_ignore_file(&from) {
                    ignore.ignore_file_removed(&from)?;
                    reload = true;
                }
                if collabignore::is_ignore_file(&to) {
                    ignore.ignore_file_modified(&to)?;
                    reload = true;
                }
                if reload {
                    load_fs_and_send_parallel(root, state, &send, false);
                }

                let ignored_from = ignore.is_ignored(&from);
                let ignored_to = ignore.is_ignored(&to);

                if !ignored_from && !ignored_to {
                    diffs.push(FsDiff::Move(
                        strip_prefix(&from, &root)?,
                        strip_prefix(&to, &root)?,
                    ))
                } else if !ignored_from {
                    // looks like file is being deleted
                    diffs.push(FsDiff::Del(strip_prefix(&from, &root)?));
                } else if !ignored_to {
                    // looks file is being created
                    let data = fs::read(&to).unwrap_or(Vec::new());
                    let stripped_path = strip_prefix(&to, &root)?;
                    match FilePerm::get(&to) {
                        Ok(perms) => {
                            diffs.push(FsDiff::Write(stripped_path.clone(), Arc::new(data)));
                            diffs.push(FsDiff::Chmod(stripped_path, perms));
                        }
                        Err(_) => (), // file may have been deleted or moved
                    }
                }
            }
            Chmod(path) => {
                let ignore = state.ignore.lock().unwrap();
                if !ignore.is_ignored(&path) {
                    match FilePerm::get(&path) {
                        Ok(perms) => diffs.push(FsDiff::Chmod(strip_prefix(&path, &root)?, perms)),
                        Err(_) => (), // file may have been deleted or moved
                    }
                }
            }
            _ => (),
        }
        for diff in diffs {
            send.send(Msg {
                body: MsgBody::Remote(RemoteMsg::FsDiff(diff)),
                source: MsgSource::Inotify,
            })
            .map_err(|err| CollabError::Error(format!("Error sending watched fs diff: {}", err)))?;
        }
    }
}
