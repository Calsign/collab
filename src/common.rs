use std::{
    collections::HashMap,
    io, net,
    path::{Path, PathBuf},
    sync::mpsc,
    sync::{Arc, Mutex},
};

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
    #[error("Error: {0}")]
    Error(String),
}

pub type Result<T> = std::result::Result<T, Error>;
pub type Reg = HashMap<PathBuf, FsReg>;
pub type Peers = HashMap<net::SocketAddr, Peer>;

pub fn strip_prefix(path: &PathBuf, prefix: &Path) -> Result<PathBuf> {
    return Ok(PathBuf::from(path.strip_prefix(prefix)?));
}

pub fn path_join(prefix: &Path, path: &Path) -> PathBuf {
    return [prefix, path].iter().collect();
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

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct IpcClientInfo {
    pub addr: net::SocketAddr,
    pub peers: Vec<PeerInfo>,
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
    Diff(FsDiff),
    AddPeer(net::SocketAddr),
    Startup(net::SocketAddr),
    LocalDisconnect,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum IpcClientMsg {
    ShutdownRequest,
    InfoRequest,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum IpcClientResponse {
    Info(IpcClientInfo),
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
    IpcClient(mpsc::Sender<IpcClientResponse>),
}

#[derive(Debug)]
pub struct Msg {
    pub body: MsgBody,
    pub source: MsgSource,
}

#[derive(Clone)]
pub struct SharedState {
    pub register: Arc<Mutex<Reg>>,
    pub peers: Arc<Mutex<Peers>>,
    pub advertised_addr: Arc<Mutex<Option<net::SocketAddr>>>,
}
