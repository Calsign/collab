use crate::common::*;
use std::{io, mem, net, sync::mpsc, thread};

const TCP_DELIM: u8 = b'\0';

fn disconnect_peer(state: &SharedState, addr: &net::SocketAddr) -> Result<()> {
    let peer_opt = state.peers.lock().unwrap().remove(&addr);
    match peer_opt {
        Some(peer) => peer.sender.send(RemoteMsg::LocalDisconnect)?,
        None => (),
    }
    return Ok(());
}

fn tcp_handler(
    stream: net::TcpStream,
    sender: mpsc::Sender<Msg>,
    receiver: mpsc::Receiver<RemoteMsg>,
    state: &SharedState,
) -> Result<()> {
    use io::{BufRead, Write};

    let addr = stream.peer_addr()?;
    println!("New peer connection: {}", addr);

    {
        let stream = stream.try_clone()?;
        thread::spawn(move || -> Result<()> {
            let mut writer = io::BufWriter::new(stream);
            loop {
                let msg = receiver.recv()?;
                match msg {
                    RemoteMsg::LocalDisconnect => return Ok(()),
                    _ => (),
                };
                let data = serde_json::to_vec(&msg)?;
                writer.write(&data[..])?;
                writer.write(&[TCP_DELIM])?;
                writer.flush()?;
            }
        });
    }

    let mut reader = io::BufReader::new(stream);
    loop {
        let mut data = Vec::new();
        match reader.read_until(TCP_DELIM, &mut data) {
            Ok(0) => {
                println!("Peer disconnected: {}", addr);
                return disconnect_peer(state, &addr);
            }
            Ok(size) => {
                let body = serde_json::from_slice(&data[..size - 1])?;
                sender.send(Msg {
                    body: MsgBody::Remote(body),
                    source: MsgSource::Peer(addr),
                })?;
            }
            Err(err) => {
                println!("Peer disconnected: {}, error: {}", addr, err);
                return disconnect_peer(state, &addr);
            }
        }
    }
}

fn add_tcp_handler(
    state: &SharedState,
    stream: net::TcpStream,
    diff_sender: mpsc::Sender<Msg>,
) -> Result<()> {
    let (sender, receiver) = mpsc::channel();
    let addr = stream.peer_addr()?;
    state.peers.lock().unwrap().insert(
        addr,
        Peer {
            sender: sender.clone(),
            info: PeerInfo {
                advertised_addr: addr,
            },
        },
    );
    let state = state.clone();
    thread::spawn(move || tcp_handler(stream, diff_sender, receiver, &state));
    return Ok(());
}

pub fn add_peer(
    addr: &net::SocketAddr,
    state: &SharedState,
    diff_send: &mpsc::Sender<Msg>,
    startup: Option<net::SocketAddr>,
) -> Result<()> {
    let stream = net::TcpStream::connect(addr)?;
    match startup {
        Some(addr) => {
            use io::Write;
            let mut writer = io::BufWriter::new(stream.try_clone()?);
            let data = serde_json::to_vec(&RemoteMsg::Startup(addr))?;
            writer.write(&data[..])?;
            writer.write(&[TCP_DELIM])?;
            writer.flush()?;
        }
        None => (),
    }
    add_tcp_handler(state, stream, diff_send.clone())?;
    return Ok(());
}

pub fn tcp_listener(
    state: &SharedState,
    diff_sender: mpsc::Sender<Msg>,
    connect: Option<net::SocketAddr>,
) -> Result<()> {
    // this picks an open port
    let socket = net::TcpListener::bind("127.0.0.1:0")?;
    let local_addr = socket.local_addr()?;
    println!("Listening for connections on {}", local_addr);

    {
        let mut advertised_addr = state.advertised_addr.lock().unwrap();
        mem::replace(&mut *advertised_addr, Some(local_addr));
    }

    match connect {
        Some(addr) => {
            println!("Attempting to connect to {}...", addr);
            add_peer(&addr, state, &diff_sender, Some(local_addr))?
        }
        None => (),
    }

    loop {
        for stream in socket.incoming() {
            match stream {
                Ok(stream) => add_tcp_handler(&state, stream, diff_sender.clone())?,
                Err(err) => println!("Failed connection: {}", err),
            }
        }
    }
}
