use crate::common::*;
use crate::ipc;
use std::{
    io::{self, BufRead},
    path::{Path, PathBuf},
    str, thread,
};

fn parse_csv(csv: &str) -> Result<BufferDiff> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .double_quote(false)
        .escape(Some(b'\\'))
        .from_reader(csv.as_bytes());
    let diff = reader.deserialize().next().unwrap()?;
    return Ok(diff);
}

fn unparse_csv(diff: &BufferDiff) -> Result<String> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .double_quote(false)
        .escape(b'\\')
        .from_writer(Vec::new());
    writer.serialize(diff)?;
    writer.flush()?;
    let vec = writer.into_inner()?;
    let csv = str::from_utf8(&vec[..])?;
    return Ok(String::from(csv));
}

pub fn attach(root: &Path, file: &Path, mode: AttachMode) -> Result<()> {
    let (sender, receiver) = ipc::client(&root)?;

    let path = file.strip_prefix(root)?;

    thread::spawn(move || -> Result<()> {
        loop {
            match receiver.recv()? {
                IpcClientResponse::BufferDiff(diff) => {
                    let text = match mode {
                        AttachMode::Json => serde_json::to_string(&diff)?,
                        AttachMode::Csv => unparse_csv(&diff)?,
                    };
                    println!("{}", text);
                }
                IpcClientResponse::LocalDisconnect | IpcClientResponse::RemoteDisconnect => {
                    return Ok(())
                }
                IpcClientResponse::Info(_) => (),
            }
        }
    });

    sender.send(IpcClientMsg::AttachRequest(PathBuf::from(&path)))?;

    loop {
        for line in io::stdin().lock().lines() {
            let line = line?;
            if line == "q" {
                // quit
                return Ok(());
            }
            let data = match mode {
                AttachMode::Json => serde_json::from_str(&line[..])?,
                AttachMode::Csv => parse_csv(&line[..])?,
            };
            sender.send(IpcClientMsg::BufferDiff(data))?;
        }
    }
}
