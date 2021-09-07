use assert_cmd::Command;
use std::thread::sleep;
use std::time::Duration;
use tempdir::TempDir;

use test_common::{common::Result, dir, file, files, rig};

#[test]
fn existing_files() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let files = dir! {
        "foobar" => file!("x"),
        "empty_dir" => dir! {},
        "another_dir" => dir! {
            "file" => file!("more text\nand another line")
        }
    };

    files.apply(&root1)?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let daemon2 = rig::connect("r2", &root2, &daemon1)?;

    sleep(Duration::from_millis(100));

    assert_eq!(files::load_dir(&root1)?, files::load_dir(&root2)?);
    assert_eq!(files, files::load_dir(&root1)?);
    assert_eq!(files, files::load_dir(&root2)?);

    return Ok(());
}

#[test]
fn added_files() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files = dir! {
        "foobar" => file!("x"),
        "empty_dir" => dir! {},
        "another_dir" => dir! {
            "file" => file!("more text\nand another line")
        }
    };

    files.apply(&root1)?;

    sleep(Duration::from_millis(100));

    assert_eq!(files::load_dir(&root1)?, files::load_dir(&root2)?);
    assert_eq!(files, files::load_dir(&root1)?);
    assert_eq!(files, files::load_dir(&root2)?);

    return Ok(());
}
