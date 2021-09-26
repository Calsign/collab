use crate::common::*;
use crate::ipc;
use context_attribute::context;
use std::{
    io::{self, BufRead},
    path::Path,
    str, thread,
};

#[context("unable to parse csv: {}", csv)]
fn parse_csv(csv: &str) -> Result<BufferDiff> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .double_quote(false)
        .escape(Some(b'\\'))
        .from_reader(csv.as_bytes());
    let diff = reader.deserialize().next().unwrap()?;
    return Ok(diff);
}

#[context("unable to unparse csv: {:?}", diff)]
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

#[context(
    "unable to attach, root: {:?}, file: {:?}, mode: {:?}",
    root,
    file,
    mode
)]
pub fn attach(root: &Path, file: &Path, desc: String, mode: AttachMode) -> Result<()> {
    let (sender, receiver) = ipc::client(&root)?;

    let path = strip_prefix(file, root)?;

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

    sender.send(IpcClientMsg::AttachRequest { path, desc })?;

    loop {
        for line in io::stdin().lock().lines() {
            let line = line?;
            if line == "q" {
                // quit
                return Ok(());
            }
            let data = match mode {
                AttachMode::Json => serde_json::from_str(&line[..])
                    .with_context(|| format!("unable to parse json: {}", &line[..]))?,
                AttachMode::Csv => parse_csv(&line[..])?,
            };
            sender.send(IpcClientMsg::BufferDiff(data))?;
        }
    }
}
