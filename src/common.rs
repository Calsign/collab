use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    fs, hash,
    hash::{Hash, Hasher},
    io, net,
    path::{Path, PathBuf},
    sync::mpsc,
    sync::{Arc, Mutex},
};

pub use relative_path::{RelativePath, RelativePathBuf};

pub use anyhow::{Context, Error, Result};
pub use context_attribute::context;

use crate::collabignore;

#[derive(thiserror::Error, Debug)]
pub enum CollabError {
    #[error("Error: {0}")]
    Error(String),
}

pub type Reg = HashMap<RelativePathBuf, FsReg>;
pub type Peers = HashMap<net::SocketAddr, Peer>;

pub fn strip_prefix(path: &Path, prefix: &Path) -> Result<RelativePathBuf> {
    return Ok(RelativePathBuf::from_path(path.strip_prefix(prefix)?)?);
}

pub fn path_join(prefix: &Path, path: &RelativePath) -> PathBuf {
    return path.to_path(prefix);
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PeerInfo {
    pub advertised_addr: net::SocketAddr,
}

#[derive(Debug)]
pub struct Peer {
    pub sender: mpsc::Sender<RemoteMsg>,
    pub info: PeerInfo,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash, Clone, Debug)]
pub struct AttachedIpcClientInfo {
    pub path: RelativePathBuf,
    pub addr: net::SocketAddr,
    pub desc: String,
}

#[derive(Clone, Debug)]
pub struct AttachedIpcClient {
    pub sender: mpsc::Sender<IpcClientResponse>,
    pub info: AttachedIpcClientInfo,
}

impl PartialEq for AttachedIpcClient {
    fn eq(&self, other: &Self) -> bool {
        return self.info == other.info;
    }
}

impl Eq for AttachedIpcClient {}

impl hash::Hash for AttachedIpcClient {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.info.hash(state);
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct IpcClientInfo {
    pub addr: net::SocketAddr,
    pub peers: Vec<PeerInfo>,
    pub attached_clients: Vec<AttachedIpcClientInfo>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct BufferDiff {
    pub pos: u32,
    pub old_len: u32,
    pub new_str: String,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
pub struct FilePerm {
    pub readonly: bool,
    pub executable: bool,
}

impl FilePerm {
    #[context("unable to get file permissions: {}", path.display())]
    pub fn get(path: &Path) -> Result<Self> {
        let perms = fs::metadata(&path)?.permissions();
        return Ok(Self {
            readonly: perms.readonly(),
            executable: if cfg!(target_family = "unix") {
                use std::os::unix::fs::PermissionsExt;
                // get user execute bit
                let user = (perms.mode() & 0o700) >> 6;
                user % 2 == 1
            } else {
                false
            },
        });
    }

    #[context("unable to set file permissions: {}", path.display())]
    pub fn set(&self, path: &Path) -> Result<()> {
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_readonly(self.readonly);
        if cfg!(target_family = "unix") {
            use std::os::unix::fs::PermissionsExt;
            let mut mode = perms.mode();
            let user = (perms.mode() & 0o700) >> 6;
            // TODO set all execute bits, not just user bit?
            if user % 2 == 0 && self.executable {
                // set execute bit
                mode += 0o100;
                perms.set_mode(mode);
            } else if user % 2 == 1 && !self.executable {
                // unset execute bit
                mode -= 0o100;
                perms.set_mode(mode);
            }
        }
        fs::set_permissions(&path, perms)?;
        return Ok(());
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum FsReg {
    File(u64, Option<FilePerm>),
    Dir,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum FsDiff {
    Write(RelativePathBuf, Arc<Vec<u8>>),
    NewDir(RelativePathBuf),
    Del(RelativePathBuf),
    Move(RelativePathBuf, RelativePathBuf),
    Chmod(RelativePathBuf, FilePerm),
}

// TODO: this type structure is a mess. clean it up, please??

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum RemoteMsg {
    FsDiff(FsDiff),
    BufferDiff(RelativePathBuf, BufferDiff),
    AddPeer(net::SocketAddr),
    Startup(net::SocketAddr),
    LocalDisconnect,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum IpcClientMsg {
    ShutdownRequest,
    InfoRequest,
    AttachRequest { path: RelativePathBuf, desc: String },
    BufferDiff(BufferDiff),
    LocalDisconnect,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum IpcClientResponse {
    Info(IpcClientInfo),
    BufferDiff(BufferDiff),
    LocalDisconnect,
    RemoteDisconnect,
}

#[derive(Clone, Debug)]
pub enum MsgBody {
    Remote(RemoteMsg),
    IpcClient(IpcClientMsg),
}

#[derive(Debug)]
pub enum MsgSource {
    Inotify,
    Peer(net::SocketAddr),
    IpcClient(mpsc::Sender<IpcClientResponse>, net::SocketAddr),
}

#[derive(Debug)]
pub struct Msg {
    pub body: MsgBody,
    pub source: MsgSource,
}

#[derive(Debug)]
pub struct AttachedClients {
    by_path: HashMap<RelativePathBuf, HashSet<AttachedIpcClient>>,
    by_addr: HashMap<net::SocketAddr, AttachedIpcClient>,
}

impl AttachedClients {
    pub fn new() -> Self {
        return AttachedClients {
            by_path: HashMap::new(),
            by_addr: HashMap::new(),
        };
    }

    pub fn add(&mut self, client: AttachedIpcClient) {
        match self.by_path.get_mut(&client.info.path) {
            Some(set) => {
                set.insert(client.clone());
            }
            None => {
                let mut set = HashSet::new();
                set.insert(client.clone());
                self.by_path.insert(client.info.path.clone(), set);
            }
        }
        self.by_addr.insert(client.info.addr, client);
    }

    pub fn remove(&mut self, client: &AttachedIpcClient) {
        match self.by_path.get_mut(&client.info.path) {
            Some(set) => {
                set.remove(&client);
                if set.is_empty() {
                    self.by_path.remove(&client.info.path);
                }
            }
            None => panic!("inconsistent attached clients data structure"),
        }
        self.by_path.remove(&client.info.path);
        self.by_addr.remove(&client.info.addr);
    }

    pub fn get_path(&self, path: &RelativePath) -> HashSet<AttachedIpcClient> {
        return match self.by_path.get(path) {
            Some(set) => set.clone(),
            None => HashSet::new(),
        };
    }

    pub fn get_addr(&self, addr: &net::SocketAddr) -> Option<AttachedIpcClient> {
        return self.by_addr.get(addr).map(AttachedIpcClient::clone);
    }

    pub fn all(&self) -> impl Iterator<Item = &AttachedIpcClient> {
        return self.by_addr.values();
    }
}

#[derive(Clone)]
pub struct SharedState {
    pub register: Arc<Mutex<Reg>>,
    pub peers: Arc<Mutex<Peers>>,
    pub attached_clients: Arc<Mutex<AttachedClients>>,
    pub ignore: Arc<Mutex<collabignore::Ignore>>,
}

#[derive(Copy, Clone, Debug)]
pub enum AttachMode {
    Json,
    Csv,
}

pub fn hash_file(data: &Arc<Vec<u8>>) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    return hasher.finish();
}
