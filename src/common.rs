use std::{
    collections::HashMap,
    io, net,
    path::{Path, PathBuf},
    sync::mpsc,
    sync::{Arc, Mutex},
};

use crate::collabignore;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error")]
    IoError(#[from] io::Error),
    #[error("Strip prefix error")]
    StripPrefixError(#[from] std::path::StripPrefixError),
    #[error("Address parsing error")]
    AddrParseError(#[from] net::AddrParseError),
    #[error("Notify error")]
    NotifyError(#[from] notify::Error),
    #[error("JSON error")]
    JsonError(#[from] serde_json::Error),
    #[error("CSV error")]
    CsvError(#[from] csv::Error),
    #[error("CSV intoinner error")]
    CsvIntoInnerError(#[from] csv::IntoInnerError<csv::Writer<Vec<u8>>>),
    #[error("Receive error")]
    RecvError(#[from] mpsc::RecvError),
    #[error("Send error (message)")]
    MsgSendError(#[from] mpsc::SendError<Msg>),
    #[error("Send error (message body)")]
    MsgBodySendError(#[from] mpsc::SendError<MsgBody>),
    #[error("Send error (remote message)")]
    RemoteMsgSendError(#[from] mpsc::SendError<RemoteMsg>),
    #[error("Send error (ipc client message)")]
    IpcClientMsgSendError(#[from] mpsc::SendError<IpcClientMsg>),
    #[error("Send error (ipc client response)")]
    IpcClientResponseSendError(#[from] mpsc::SendError<IpcClientResponse>),
    #[error("Gitignore error")]
    GitignoreError(#[from] ignore::Error),
    #[error("UTF-8 parsing error")]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error("Error: {0}")]
    Error(String),
}

pub type Result<T> = std::result::Result<T, Error>;
pub type Reg = HashMap<PathBuf, FsReg>;
pub type Peers = HashMap<net::SocketAddr, Peer>;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PeerInfo {
    pub advertised_addr: net::SocketAddr,
}

#[derive(Debug)]
pub struct Peer {
    pub sender: mpsc::Sender<RemoteMsg>,
    pub info: PeerInfo,
}

#[derive(Clone, Debug)]
pub struct AttachedIpcClient {
    pub path: PathBuf,
    pub addr: net::SocketAddr,
    pub sender: mpsc::Sender<IpcClientResponse>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct IpcClientInfo {
    pub addr: net::SocketAddr,
    pub peers: Vec<PeerInfo>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct BufferDiff {
    pub pos: u32,
    pub before: String,
    pub after: String,
}

#[derive(PartialEq, Eq, Debug)]
pub enum FsReg {
    File(Arc<Vec<u8>>),
    Dir,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum FsDiff {
    Write(PathBuf, Arc<Vec<u8>>),
    NewDir(PathBuf),
    Del(PathBuf),
    Move(PathBuf, PathBuf),
}

// TODO: this type structure is a mess. clean it up, please??

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum RemoteMsg {
    FsDiff(FsDiff),
    BufferDiff(BufferDiff),
    AddPeer(net::SocketAddr),
    Startup(net::SocketAddr),
    LocalDisconnect,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum IpcClientMsg {
    ShutdownRequest,
    InfoRequest,
    AttachRequest(PathBuf),
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
    by_path: HashMap<PathBuf, AttachedIpcClient>,
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
        self.by_path.insert(client.path.clone(), client.clone());
        self.by_addr.insert(client.addr, client);
    }

    pub fn remove(&mut self, client: &AttachedIpcClient) {
        self.by_path.remove(&client.path);
        self.by_addr.remove(&client.addr);
    }

    pub fn get_path(&self, path: &Path) -> Option<AttachedIpcClient> {
        return self
            .by_path
            .get(&PathBuf::from(path))
            .map(AttachedIpcClient::clone);
    }

    pub fn get_addr(&self, addr: &net::SocketAddr) -> Option<AttachedIpcClient> {
        return self.by_addr.get(addr).map(AttachedIpcClient::clone);
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
