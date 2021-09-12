mod attach;
mod cli;
mod collabignore;
mod common;
mod fs_watcher;
mod ipc;
mod tcp;

use crate::common::*;
use std::{
    collections::HashMap,
    net,
    path::PathBuf,
    process,
    sync::mpsc,
    sync::{Arc, Mutex},
    thread,
};

#[context("unable to send startup, source: {}, advertised: {}, root: {}",
          source_addr, advertised_addr, root.display())]
fn send_startup(
    source_addr: net::SocketAddr,
    advertised_addr: net::SocketAddr,
    root: &PathBuf,
    state: &SharedState,
) -> Result<()> {
    let mut peers = state.peers.lock().unwrap();

    let sender = peers[&source_addr].sender.clone();

    // update advertised address
    peers.insert(
        source_addr,
        Peer {
            sender: sender.clone(),
            info: PeerInfo { advertised_addr },
        },
    );

    // inform new peer of other peers
    for peer in peers.values() {
        // don't tell the new connection about itself!
        if peer.info.advertised_addr != advertised_addr {
            sender.send(RemoteMsg::AddPeer(peer.info.advertised_addr))?;
        }
    }

    let diffs = fs_watcher::load_fs(&root, state)?;
    let mut register = state.register.lock().unwrap();
    for diff in diffs {
        diff.register(&mut register)?;
        sender.send(RemoteMsg::FsDiff(diff))?;
    }

    return Ok(());
}

#[context("unable to start server, connect: {:?}", connect)]
fn server(root: PathBuf, connect: Option<net::SocketAddr>) -> Result<()> {
    let state = SharedState {
        register: Arc::new(Mutex::new(HashMap::new())),
        peers: Arc::new(Mutex::new(HashMap::new())),
        attached_clients: Arc::new(Mutex::new(AttachedClients::new())),
        ignore: Arc::new(Mutex::new(collabignore::Ignore::new(&root))),
    };

    if ipc::has_active_session(&root)? {
        return Err(CollabError::Error(
            "A session is already started in this directory!".to_string(),
        )
        .into());
    }

    {
        let root = root.clone();
        ctrlc::set_handler(move || {
            match ipc::daemon_cleanup(&root) {
                Ok(()) => (),
                Err(err) => {
                    eprintln!("Error cleaning up: {}", err);
                }
            }
            process::exit(0);
        })
        .expect("Failed to set sigint handler");
    }

    let (msg_sender, msg_receiver) = mpsc::channel();

    {
        let (root, msg_sender) = (root.clone(), msg_sender.clone());
        thread::spawn(move || ipc::daemon(&root, msg_sender));
    }

    let addr = tcp::tcp_listener(&state, &msg_sender, connect)?;
    println!("Listening for connections on {}", addr);

    let _fs_watcher = {
        let (root, msg_sender, state) = (root.clone(), msg_sender.clone(), state.clone());
        thread::spawn(move || fs_watcher::watch_fs(&root, &state, msg_sender).expect("whoops"));
    };

    loop {
        match msg_receiver.recv() {
            Ok(msg) => {
                println!("msg: {:?}", msg); // for testing
                match (msg.body, msg.source) {
                    (MsgBody::Remote(RemoteMsg::FsDiff(diff)), msg_source) => {
                        let mut register = state.register.lock().unwrap();
                        let changes_register = diff.changes_register(&mut register);

                        if changes_register {
                            diff.register(&mut register)?;

                            match msg_source {
                                MsgSource::Peer(_) => diff.apply(&root)?,
                                MsgSource::Inotify => {
                                    for peer in state.peers.lock().unwrap().values() {
                                        peer.sender.send(RemoteMsg::FsDiff(diff.clone()))?;
                                    }
                                }
                                MsgSource::IpcClient(_, _) => (),
                            }
                        }
                    }
                    (
                        MsgBody::IpcClient(IpcClientMsg::AttachRequest { path, desc }),
                        MsgSource::IpcClient(sender, addr),
                    ) => {
                        // add the new client
                        state
                            .attached_clients
                            .lock()
                            .unwrap()
                            .add(AttachedIpcClient {
                                info: AttachedIpcClientInfo { path, desc, addr },
                                sender,
                            });
                    }
                    (
                        MsgBody::IpcClient(IpcClientMsg::LocalDisconnect),
                        MsgSource::IpcClient(_, addr),
                    ) => {
                        // remove the client
                        let mut clients = state.attached_clients.lock().unwrap();
                        match clients.get_addr(&addr) {
                            Some(client) => clients.remove(&client),
                            None => (),
                        };
                    }
                    (
                        MsgBody::IpcClient(IpcClientMsg::BufferDiff(diff)),
                        MsgSource::IpcClient(_, addr),
                    ) => {
                        let clients = state.attached_clients.lock().unwrap();
                        let path = clients.get_addr(&addr).unwrap().info.path;
                        for client in clients.get_path(&path) {
                            if addr != client.info.addr {
                                client
                                    .sender
                                    .send(IpcClientResponse::BufferDiff(diff.clone()))?;
                            }
                        }
                        let peers = state.peers.lock().unwrap();
                        for peer in peers.values() {
                            peer.sender
                                .send(RemoteMsg::BufferDiff(path.clone(), diff.clone()))?;
                        }
                    }
                    (MsgBody::Remote(RemoteMsg::BufferDiff(path, diff)), MsgSource::Peer(peer)) => {
                        let clients = state.attached_clients.lock().unwrap();
                        for client in clients.get_path(&path) {
                            client
                                .sender
                                .send(IpcClientResponse::BufferDiff(diff.clone()))?;
                        }
                    }
                    (MsgBody::Remote(RemoteMsg::AddPeer(addr)), _) => {
                        tcp::add_peer(&addr, &state, &msg_sender, None)?
                    }
                    (
                        MsgBody::Remote(RemoteMsg::Startup(advertised_addr)),
                        MsgSource::Peer(source_addr),
                    ) => {
                        send_startup(source_addr, advertised_addr, &root, &state)?;
                    }
                    (
                        MsgBody::IpcClient(IpcClientMsg::ShutdownRequest),
                        MsgSource::IpcClient(_, _),
                    ) => {
                        println!("Shutting down daemon...");
                        return ipc::daemon_cleanup(&root);
                    }
                    (
                        MsgBody::IpcClient(IpcClientMsg::InfoRequest),
                        MsgSource::IpcClient(response_sender, _),
                    ) => {
                        let peers = state
                            .peers
                            .lock()
                            .unwrap()
                            .values()
                            .map(|peer| peer.info.clone())
                            .collect();
                        let attached_clients = state
                            .attached_clients
                            .lock()
                            .unwrap()
                            .all()
                            .map(|client| client.info.clone())
                            .collect();
                        response_sender.send(IpcClientResponse::Info(IpcClientInfo {
                            addr,
                            peers,
                            attached_clients,
                        }))?;
                    }
                    _ => (),
                };
            }
            Err(err) => eprintln!("{:?}", err),
        }
    }
}

fn handle_command(root: PathBuf, command: cli::CliCommand) -> Result<()> {
    use cli::CliCommand::*;
    match command {
        Start { connect } => server(root, connect)?,
        Stop => ipc::client_send_stop(&root)?,
        Info => {
            let info = ipc::client_get_info(&root)?;
            println!("Address: {}", info.addr);
            println!("Peers ({} total):", info.peers.len());
            for peer in info.peers {
                println!("  {}", peer.advertised_addr);
            }
            println!("Attached clients ({} total):", info.attached_clients.len());
            for client in info.attached_clients {
                println!("  {}: {}", client.desc, client.path.as_str());
            }
        }
        List => {
            let active_sessions = ipc::get_active_sessions()?;
            println!("Active sessions ({} total):", active_sessions.len());
            for session_path in active_sessions {
                println!("{}", session_path.display());
            }
        }
        Attach { file, desc, mode } => attach::attach(&root, &file, desc, mode)?,
    };

    return Ok(());
}

fn main() -> Result<()> {
    use cli::Cli;
    let Cli { root, command } = cli::parse_cli()?;
    return handle_command(root, command);
}

// current problems:
//  - don't load entire file into memory, send it by streaming instead?
//  - possibly place a hard limit on size of tracked files? (1 MB?)
//  - interface for sending/receiving diffs
//  - connect without having to delete existing files in directory
//  - operational transformation
//  - editor integration
//  - encryption with libsignal-protocol?
//  - maybe other things? I forget
