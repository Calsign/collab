use colored::*;
use lazy_static::lazy_static;
use regex::Regex;
use relative_path::{RelativePath, RelativePathBuf};
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tempdir::TempDir;

use crate::common;
use crate::util;

pub fn spawn<I: IntoIterator<Item = S>, S: AsRef<OsStr>, P: AsRef<Path>>(
    args: I,
    root: P,
) -> process::Command {
    let cargo_bin = util::cargo_bin("collab");
    let mut cmd = process::Command::new(cargo_bin);
    cmd.current_dir(root);
    cmd.args(args);
    return cmd;
}

pub struct Daemon {
    pub id: String,
    daemon: process::Child,
    pub root: PathBuf,
    address: String,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let mut stop = spawn(&["stop"], &self.root).spawn().unwrap();

        let wait_daemon = self.daemon.wait();
        let wait_stop = stop.wait();

        // if we have already failed, just give up
        if !thread::panicking() {
            // wait on the daemon first so that we see the correct error
            assert!(wait_daemon.unwrap().success());
            assert!(wait_stop.unwrap().success());
        }
    }
}

fn spawn_daemon<P: AsRef<Path>>(
    id: &str,
    root: &P,
    connect: Option<&Daemon>,
) -> common::Result<Daemon> {
    let mut args = Vec::new();
    args.push("start");
    match connect {
        Some(peer) => {
            args.push("-c");
            args.push(&peer.address);
        }
        None => {}
    }

    let mut daemon = spawn(args, &root)
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn()?;

    let mut stdout = BufReader::new(daemon.stdout.take().unwrap()).lines();
    let stderr = BufReader::new(daemon.stderr.take().unwrap()).lines();

    lazy_static! {
        // extract IP address and port, e.g. 127.0.0.1:12345
        // TODO: perhaps there is a better way to do this?
        static ref RE: Regex =
            Regex::new(r"[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+").unwrap();
    }

    fn echo_line(line: &str, id: &str, err: bool) {
        if err {
            eprintln!("DAEMON {} stderr: {}", id, line.red());
        } else {
            println!("DAEMON {} stdout: {}", id, line);
        }
    }

    let address = loop {
        // wait until the address gets printed
        let line = stdout.next().unwrap().unwrap();
        echo_line(&line, &id, false);
        match RE.captures(&line).map(|m| m.get(0)) {
            Some(Some(add)) => break add.as_str().to_string(),
            _ => continue,
        }
    };

    {
        let id = id.to_string();
        thread::spawn(move || {
            for line in stdout {
                echo_line(&line.unwrap(), &id, false);
            }
        });
    }

    {
        let id = id.to_string();
        thread::spawn(move || {
            for line in stderr {
                echo_line(&line.unwrap(), &id, true);
            }
        });
    }

    return Ok(Daemon {
        id: id.to_string(),
        daemon,
        root: root.as_ref().to_path_buf(),
        address,
    });
}

pub fn daemon<P: AsRef<Path>>(id: &str, root: &P) -> common::Result<Daemon> {
    return Ok(spawn_daemon(id, root, None)?);
}

pub fn connect<P: AsRef<Path>>(id: &str, root: &P, peer: &Daemon) -> common::Result<Daemon> {
    return Ok(spawn_daemon(id, root, Some(peer))?);
}

pub struct Attach<'a> {
    daemon: &'a Daemon,
    path: RelativePathBuf,
    process: process::Child,
    stdout: Arc<Mutex<VecDeque<String>>>,
    stderr: Arc<Mutex<VecDeque<String>>>,
}

impl<'a> Drop for Attach<'a> {
    fn drop(&mut self) {
        let res = self.send("q");
        if !thread::panicking() {
            res.unwrap();
        }
        let wait = self.process.wait();
        if !thread::panicking() {
            assert!(wait.unwrap().success());
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
pub struct BufferDiff {
    pub pos: u32,
    pub old_len: u32,
    pub new_str: String,
}

impl BufferDiff {
    pub fn new<S: Into<String>>(pos: u32, old_len: u32, new_str: S) -> Self {
        return Self {
            pos,
            old_len,
            new_str: new_str.into(),
        };
    }
}

impl<'a> Attach<'a> {
    pub fn send<D: AsRef<[u8]>>(&mut self, data: D) -> common::Result<()> {
        let stdin = self.process.stdin.as_mut().unwrap();
        stdin.write(data.as_ref())?;
        stdin.write(b"\n")?;
        stdin.flush()?;
        return Ok(());
    }

    pub fn pop_stdout(&mut self) -> Option<String> {
        return self.stdout.lock().unwrap().pop_front();
    }

    pub fn pop_stderr(&mut self) -> Option<String> {
        return self.stderr.lock().unwrap().pop_front();
    }

    pub fn peek_stdout(&mut self) -> Option<String> {
        return self.stdout.lock().unwrap().front().map(String::clone);
    }

    pub fn peek_stderr(&mut self) -> Option<String> {
        return self.stderr.lock().unwrap().front().map(String::clone);
    }

    pub fn send_diff(&mut self, diff: &BufferDiff) -> common::Result<()> {
        return self.send(serde_json::to_string(diff)?);
    }

    pub fn pop_diff(&mut self) -> common::Result<Option<BufferDiff>> {
        return Ok(match self.pop_stdout() {
            Some(s) => serde_json::from_str(&s)?,
            None => None,
        });
    }
}

pub fn attach<'a, P: AsRef<RelativePath>>(
    daemon: &'a Daemon,
    path: P,
) -> common::Result<Attach<'a>> {
    let path_ref = path.as_ref();
    let mut process = spawn(
        &["attach", "--description", "", "--file", path_ref.as_str()],
        &daemon.root,
    )
    .stdout(process::Stdio::piped())
    .stderr(process::Stdio::piped())
    .stdin(process::Stdio::piped())
    .spawn()?;

    let stdout = BufReader::new(process.stdout.take().unwrap()).lines();
    let stderr = BufReader::new(process.stderr.take().unwrap()).lines();

    let stdout_deque = Arc::new(Mutex::new(VecDeque::new()));
    let stderr_deque = Arc::new(Mutex::new(VecDeque::new()));

    fn echo_line(line: &str, id: &str, path: &RelativePath, err: bool) {
        if err {
            eprintln!("DAEMON {} stderr for {}: {}", id, path, line.red());
        } else {
            println!("DAEMON {} stdout for {}: {}", id, path, line);
        }
    }

    {
        let (id, path, stdout_deque) = (
            daemon.id.clone(),
            path_ref.to_relative_path_buf(),
            stdout_deque.clone(),
        );
        thread::spawn(move || {
            for line in stdout {
                let line = line.unwrap();
                echo_line(&line, &id, &path, false);
                stdout_deque.lock().unwrap().push_back(line);
            }
        });
    }

    {
        let (id, path, stderr_deque) = (
            daemon.id.clone(),
            path_ref.to_relative_path_buf(),
            stderr_deque.clone(),
        );
        thread::spawn(move || {
            for line in stderr {
                let line = line.unwrap();
                echo_line(&line, &id, &path, true);
                stderr_deque.lock().unwrap().push_back(line);
            }
        });
    }

    return Ok(Attach {
        daemon: &daemon,
        path: path_ref.to_relative_path_buf(),
        process: process,
        stdout: stdout_deque,
        stderr: stderr_deque,
    });
}

pub fn tempdir() -> common::Result<TempDir> {
    return Ok(TempDir::new("collab_test")?);
}

pub fn wait() {
    thread::sleep(Duration::from_millis(200));
}

#[macro_export]
macro_rules! path(
    { $($segment:expr),+ } => {
        {
            let mut base = ::std::path::PathBuf::new();
            $(
                base.push($segment);
            )*
            base
        }
    }
);
