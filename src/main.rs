mod cli;
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

    let diffs = fs_watcher::load_fs(&root)?;
    let mut register = state.register.lock().unwrap();
    for diff in diffs {
        diff.register(&mut register)?;
        sender.send(RemoteMsg::Diff(diff))?;
    }

    return Ok(());
}

fn server(root: PathBuf, connect: Option<net::SocketAddr>) -> Result<()> {
    let state = SharedState {
        register: Arc::new(Mutex::new(HashMap::new())),
        peers: Arc::new(Mutex::new(HashMap::new())),
        advertised_addr: Arc::new(Mutex::new(None)),
    };

    {
        let root = root.clone();
        ctrlc::set_handler(move || {
            ipc::daemon_cleanup(&root).expect("Failed to clean up!");
            process::exit(0);
        })
        .expect("Failed to set sigint handler");
    }

    let (msg_sender, msg_receiver) = mpsc::channel();

    {
        let (root, msg_sender) = (root.clone(), msg_sender.clone());
        thread::spawn(move || ipc::daemon(&root, msg_sender));
    }

    {
        let (state, msg_sender) = (state.clone(), msg_sender.clone());
        thread::spawn(move || tcp::tcp_listener(&state, msg_sender, connect));
    }

    let _fs_watcher = {
        let (root, msg_sender, state) = (root.clone(), msg_sender.clone(), state.clone());
        thread::spawn(move || fs_watcher::watch_fs(&root, &state, msg_sender).expect("whoops"));
    };

    loop {
        match msg_receiver.recv() {
            Ok(msg) => {
                println!("msg: {:?}", msg); // for testing
                match (msg.body, msg.source) {
                    (MsgBody::Remote(RemoteMsg::Diff(diff)), msg_source) => {
                        let mut register = state.register.lock().unwrap();
                        let changes_register = diff.changes_register(&mut register);

                        if changes_register {
                            diff.register(&mut register)?;

                            match msg_source {
                                MsgSource::Peer(_) => diff.apply(&root)?,
                                MsgSource::Inotify => {
                                    for peer in state.peers.lock().unwrap().values() {
                                        peer.sender.send(RemoteMsg::Diff(diff.clone()))?;
                                    }
                                }
                                MsgSource::IpcClient(_) => (),
                            }
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
                        MsgSource::IpcClient(_),
                    ) => {
                        println!("Shutting down daemon...");
                        return ipc::daemon_cleanup(&root);
                    }
                    (
                        MsgBody::IpcClient(IpcClientMsg::InfoRequest),
                        MsgSource::IpcClient(response_sender),
                    ) => {
                        let addr = state
                            .advertised_addr
                            .lock()
                            .unwrap()
                            .expect("advertised addr not populated!");
                        let peers = state
                            .peers
                            .lock()
                            .unwrap()
                            .values()
                            .map(|peer| peer.info.clone())
                            .collect();
                        response_sender
                            .send(IpcClientResponse::Info(IpcClientInfo { addr, peers }))?;
                    }
                    _ => (),
                };
            }
            Err(err) => println!("{:?}", err),
        }
    }
}

fn main() -> Result<()> {
    use cli::{Cli, CliCommand::*};

    let Cli { root, command } = cli::parse_cli()?;

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
        }
        List => {
            let active_sessions = ipc::get_active_sessions()?;
            println!("Active sessions ({} total):", active_sessions.len());
            for session_path in active_sessions {
                println!("{}", session_path.display());
            }
        }
    };

    return Ok(());
}

// current problems:
//  - ignore certain paths (based on .gitignore)
//  - sync file permissions (e.g. execute bit) (there may be trouble supporting Windows...)
//  - don't load entire file into memory, send it by streaming instead?
//  - interface for sending/receiving diffs
//  - operational transformation
//  - editor integration
//  - encryption with libsignal-protocol?
//  - maybe other things? I forget
