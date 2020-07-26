use crate::common::*;
use std::{env, fs, net, path::PathBuf};

pub enum CliCommand {
    Start { connect: Option<net::SocketAddr> },
    Stop,
    Info,
    List,
}

pub struct Cli {
    pub root: PathBuf,
    pub command: CliCommand,
}

pub fn parse_cli() -> Result<Cli> {
    use clap::{App, Arg, SubCommand};

    let matches = App::new("collab")
        .about("Google Docs for code")
        .arg(
            Arg::with_name("root")
                .short("r")
                .long("root")
                .value_name("PATH")
                .help("Root file or directory")
                .takes_value(true),
        )
        .subcommand(
            SubCommand::with_name("start")
                .about("Start collab daemon")
                .arg(
                    Arg::with_name("connect")
                        .short("c")
                        .long("connect")
                        .value_name("ADDRESS:PORT")
                        .help("Instance to connect to")
                        .takes_value(true)
                        .validator(|str| match net::ToSocketAddrs::to_socket_addrs(&str) {
                            Ok(_) => Ok(()),
                            Err(_) => Err("invalid address".to_string()),
                        }),
                ),
        )
        .subcommand(SubCommand::with_name("stop").about("Stop the current session"))
        .subcommand(SubCommand::with_name("info").about("Print info for current session"))
        .subcommand(SubCommand::with_name("list").about("List all active sessions"))
        .get_matches();

    let root = matches
        .value_of("root")
        .map(PathBuf::from)
        .unwrap_or(env::current_dir()?)
        .canonicalize()?;

    if root.exists() {
        if !fs::metadata(&root)?.is_dir() {
            return Err(Error::Error("Root must be a directory".to_string()));
        }
    } else {
        println!("Creating new directory...");
        fs::create_dir(&root)?;
    }

    let command = match matches.subcommand() {
        ("start", Some(matches)) => {
            let connect: Option<net::SocketAddr> = match matches.value_of("connect") {
                Some(str) => {
                    if fs::read_dir(&root)?.next().is_some() {
                        return Err(Error::Error(
                            "Root directory must be non-empty when connecting to an existing collab"
                                .to_string(),
                        ));
                    }
                    Some(str.parse()?)
                }
                None => None,
            };
            CliCommand::Start { connect }
        }
        ("stop", _) => CliCommand::Stop,
        ("info", _) | (_, None) => CliCommand::Info,
        ("list", _) => CliCommand::List,
        (subcommand, Some(_)) => panic!("unrecognized command: {}", subcommand),
    };

    return Ok(Cli { root, command });
}
