use crate::common::*;
use std::{
    env, fs, io, net,
    path::{Path, PathBuf},
    process,
    sync::mpsc,
    thread,
};

const TCP_DELIM: u8 = b'\0';

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct TmpData {
    pub port: u16,
    pub pid: i32,
}

#[derive(Clone, Debug)]
struct TmpKey {
    path: PathBuf,
}

impl TmpKey {
    fn exists(&self) -> bool {
        return self.path.exists();
    }
}

fn get_temp_dir() -> PathBuf {
    let mut buf = env::temp_dir();
    buf.push("collab");
    return buf;
}

/// Gets the key for a particular directory.
#[context("unable to get temp key: {}", path.display())]
fn get_key(path: &Path) -> Result<TmpKey> {
    match path.as_os_str().to_str() {
        Some(path) => {
            let mut buf = get_temp_dir();
            buf.push(path.replace("/", "!"));
            return Ok(TmpKey { path: buf });
        }
        None => return Err(CollabError::Error(format!("Path not unicode: {:?}", path)).into()),
    }
}

/// Checks to make sure the process associated with the given pid is still alive.
/// If it is alive, returns true; if it is dead, deletes the key file and returns false.
#[context("unable to verify key: {}", key.path.display())]
fn verify_key(key: &TmpKey) -> Result<bool> {
    let data = load_data(key)?;
    if cfg!(target_family = "unix") {
        let res: i32;
        unsafe {
            res = libc::kill(data.pid, 0);
        };
        return Ok(if res == 0 {
            true
        } else {
            fs::remove_file(&key.path)?;
            false
        });
    } else {
        // TODO: test process liveness on other platforms
        return Ok(true);
    }
}

/// Traverses upward in the directory structure until a directory
/// with an active key is found.
#[context("unable to find key: {}", path.display())]
fn find_key(path: &Path) -> Result<Option<TmpKey>> {
    for ancestor in path.ancestors() {
        let key = get_key(&ancestor)?;
        if key.exists() {
            return Ok(if verify_key(&key)? { Some(key) } else { None });
        }
    }
    return Ok(None);
}

pub fn has_active_session(path: &Path) -> Result<bool> {
    return Ok(find_key(path)?.is_some());
}

#[context("unable to get active sessions")]
pub fn get_active_sessions() -> Result<Vec<PathBuf>> {
    let mut list = Vec::new();
    let temp_dir = get_temp_dir();

    if temp_dir.exists() {
        for entry in fs::read_dir(temp_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                match entry.file_name().to_str() {
                    Some(name) => {
                        if verify_key(&TmpKey { path })? {
                            let replaced = name.replace("!", "/");
                            list.push(PathBuf::from(replaced));
                        }
                    }
                    None => (),
                }
            }
        }
    }

    return Ok(list);
}

/// Used by the client.
#[context("unable to load temp key data: {}", key.path.display())]
fn load_data(key: &TmpKey) -> Result<TmpData> {
    if !key.exists() {
        panic!("key does not exist: {:?}", key);
    }

    let buf = fs::read(&key.path)?;
    let data = serde_json::from_slice(&buf[..])?;
    return Ok(data);
}

/// Used by the daemon.
#[context("unable to write temp key data: {}", key.path.display())]
fn write_data(key: &TmpKey, data: TmpData) -> Result<()> {
    let buf = serde_json::to_vec(&data)?;
    fs::create_dir_all(get_temp_dir())?;
    fs::write(&key.path, buf)?;
    return Ok(());
}

#[context("unable to remove temp key data: {}", key.path.display())]
fn remove_data(key: &TmpKey) -> Result<()> {
    fs::remove_file(&key.path)?;
    return Ok(());
}

#[context("unable to start ipc daemon thread")]
fn daemon_thread(stream: net::TcpStream, sender: mpsc::Sender<Msg>) -> Result<()> {
    use io::{BufRead, Write};

    let addr = stream.peer_addr()?;

    let mut reader = io::BufReader::new(stream.try_clone()?);
    let mut writer = io::BufWriter::new(stream);

    let (response_sender, response_receiver) = mpsc::channel();

    thread::spawn(move || -> Result<()> {
        loop {
            let msg = response_receiver.recv()?;
            match msg {
                IpcClientResponse::LocalDisconnect => return Ok(()),
                _ => (),
            };
            let data = serde_json::to_vec(&msg)?;
            writer.write(&data[..])?;
            writer.write(&[TCP_DELIM])?;
            writer.flush()?;
        }
    });

    loop {
        let mut data = Vec::new();
        match reader.read_until(TCP_DELIM, &mut data) {
            Ok(0) => {
                response_sender.send(IpcClientResponse::LocalDisconnect)?;
                sender
                    .send(Msg {
                        body: MsgBody::IpcClient(IpcClientMsg::LocalDisconnect),
                        source: MsgSource::IpcClient(response_sender.clone(), addr),
                    })
                    .map_err(|err| {
                        CollabError::Error(format!(
                            "Error sending received IPC client disconnect: {}",
                            err
                        ))
                    })?;
                return Ok(());
            }
            Ok(size) => {
                let msg = serde_json::from_slice(&data[..size - 1])?;
                sender
                    .send(Msg {
                        body: MsgBody::IpcClient(msg),
                        source: MsgSource::IpcClient(response_sender.clone(), addr),
                    })
                    .map_err(|err| {
                        CollabError::Error(format!(
                            "Error sending received IPC client message: {}",
                            err
                        ))
                    })?;
            }
            Err(err) => {
                eprintln!("ipc error: {}", err);
                response_sender.send(IpcClientResponse::LocalDisconnect)?;
                return Err(err.into());
            }
        }
    }
}

#[context("unable to start ipc daemon: {}", root.display())]
pub fn daemon(root: &Path, sender: mpsc::Sender<Msg>) -> Result<()> {
    use std::convert::TryFrom;

    let key = get_key(&root)?;

    let socket = net::TcpListener::bind("127.0.0.1:0")?;
    let addr = socket.local_addr()?;

    println!("writing data: {:?}", key);
    write_data(
        &key,
        TmpData {
            port: addr.port(),
            pid: i32::try_from(process::id()).unwrap(),
        },
    )?;

    loop {
        for stream in socket.incoming() {
            match stream {
                Ok(stream) => {
                    let sender = sender.clone();
                    thread::spawn(move || daemon_thread(stream, sender));
                }
                Err(err) => eprintln!("Failed ipc connection: {}", err),
            }
        }
    }
}

#[context("unable to clean up ipc daemon: {}", root.display())]
pub fn daemon_cleanup(root: &Path) -> Result<()> {
    let key = get_key(&root)?;
    return remove_data(&key);
}

#[context("unable to start ipc client: {}", root.display())]
pub fn client(
    root: &Path,
) -> Result<(
    mpsc::Sender<IpcClientMsg>,
    mpsc::Receiver<IpcClientResponse>,
)> {
    use io::{BufRead, Write};
    use net::*;

    let key = match find_key(root)? {
        Some(key) => key,
        None => return Err(CollabError::Error("No session in this directory".to_string()).into()),
    };

    let data = load_data(&key)?;
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), data.port);
    let stream = TcpStream::connect(&addr)?;

    let mut reader = io::BufReader::new(stream.try_clone()?);
    let mut writer = io::BufWriter::new(stream);

    let (request_sender, request_receiver) = mpsc::channel();
    let (response_sender, response_receiver) = mpsc::channel();

    thread::spawn(move || -> Result<()> {
        loop {
            let request = request_receiver.recv()?;
            let data = serde_json::to_vec(&request)?;
            writer.write(&data[..])?;
            writer.write(&[TCP_DELIM])?;
            writer.flush()?;
        }
    });

    thread::spawn(move || -> Result<()> {
        loop {
            let mut data = Vec::new();
            match reader.read_until(TCP_DELIM, &mut data) {
                Ok(0) => {
                    response_sender.send(IpcClientResponse::RemoteDisconnect)?;
                    return Ok(());
                }
                Ok(size) => {
                    let response = serde_json::from_slice(&data[..size - 1])?;
                    response_sender.send(response)?;
                }
                Err(err) => {
                    eprintln!("ipc error: {}", err);
                    response_sender.send(IpcClientResponse::RemoteDisconnect)?;
                    return Err(err.into());
                }
            }
        }
    });

    return Ok((request_sender, response_receiver));
}

#[context("unable to send client stop: {}", root.display())]
pub fn client_send_stop(root: &Path) -> Result<()> {
    let (request_sender, response_receiver) = client(root)?;
    request_sender.send(IpcClientMsg::ShutdownRequest)?;
    // wait for disconnect to make sure request goes through
    return match response_receiver.recv()? {
        IpcClientResponse::RemoteDisconnect => Ok(()),
        _ => Err(CollabError::Error("Daemon sent bad response".to_string()).into()),
    };
}

#[context("unable to set client info: {}", root.display())]
pub fn client_get_info(root: &Path) -> Result<IpcClientInfo> {
    let (request_sender, response_receiver) = client(root)?;
    request_sender.send(IpcClientMsg::InfoRequest)?;
    return match response_receiver.recv()? {
        IpcClientResponse::Info(info) => Ok(info),
        _ => Err(CollabError::Error("Daemon sent bad response".to_string()).into()),
    };
}
