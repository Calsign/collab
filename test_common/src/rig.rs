use assert_cmd;
use lazy_static::lazy_static;
use regex::Regex;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
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

        // if we have already failed, just give up
        if !thread::panicking() {
            // wait on the daemon first so that we see the correct error
            assert!(self.daemon.wait().unwrap().success());
            assert!(stop.wait().unwrap().success());
        }
    }
}

fn spawn_daemon<P: AsRef<Path>>(
    id: &str,
    root: P,
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

    let mut daemon = spawn(args, &root).stdout(process::Stdio::piped()).spawn()?;

    let stdout = daemon.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();

    lazy_static! {
        // extract IP address and port, e.g. 127.0.0.1:12345
        // TODO: perhaps there is a better way to do this?
        static ref RE: Regex =
            Regex::new(r".* ([0-9]+\.[0-9]+\.[0-9]+\.[0-9]:[0-9]+)").unwrap();
    }

    fn echo_line(line: &str, id: &str) {
        println!("DAEMON {}: {}", id, line);
    }

    let line = lines.next().unwrap().unwrap();
    let address = RE.captures(&line).unwrap().get(1).unwrap().as_str();
    echo_line(&line, &id);

    {
        let id = id.to_string();
        thread::spawn(move || {
            for line in lines {
                echo_line(&line.unwrap(), &id);
            }
        });
    }

    return Ok(Daemon {
        id: id.to_string(),
        daemon,
        root: root.as_ref().to_path_buf(),
        address: address.to_string(),
    });
}

pub fn daemon<P: AsRef<Path>>(id: &str, root: P) -> common::Result<Daemon> {
    return spawn_daemon(id, root, None);
}

pub fn connect<P: AsRef<Path>>(id: &str, root: P, peer: &Daemon) -> common::Result<Daemon> {
    return spawn_daemon(id, root, Some(peer));
}