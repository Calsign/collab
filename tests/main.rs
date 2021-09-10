use std::fs;

use test_common::{common::Result, dir, file, files, path, rig};

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
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    rig::wait();

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
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files = dir! {
        "foobar" => file!("x"),
        "empty_dir" => dir! {},
        "another_dir" => dir! {
            "file" => file!("more text\nand another line")
        }
    };

    files.apply(&root1)?;

    rig::wait();

    assert_eq!(files::load_dir(&root1)?, files::load_dir(&root2)?);
    assert_eq!(files, files::load_dir(&root1)?);
    assert_eq!(files, files::load_dir(&root2)?);

    return Ok(());
}

#[test]
fn send_back() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files1 = dir! {
        "x" => file!("x")
    };

    files1.apply(&root1)?;

    rig::wait();

    assert_eq!(files1, files::load_dir(&root1)?);
    assert_eq!(files1, files::load_dir(&root2)?);

    let files2 = dir! {
        "x" => file!("x"),
        "y" => file!("y")
    };

    files2.apply(&root2)?;

    rig::wait();

    assert_eq!(files2, files::load_dir(&root1)?);
    assert_eq!(files2, files::load_dir(&root2)?);

    return Ok(());
}

#[test]
fn null_byte() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files = dir! {
        "foobar" => file!(r"data\0\0more data")
    };

    files.apply(&root1)?;

    rig::wait();

    assert_eq!(files, files::load_dir(&root1)?);
    assert_eq!(files, files::load_dir(&root2)?);

    return Ok(());
}

#[test]
#[cfg(target_family = "unix")]
fn chmod() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files = dir! {
        "foobar" => file!("foobar")
    };

    files.apply(&root1)?;

    rig::wait();

    assert_eq!(files, files::load_dir(&root1)?);
    assert_eq!(files, files::load_dir(&root2)?);

    let path = path!(&root1, "foobar");
    let mut perms1 = fs::metadata(&path)?.permissions();
    perms1.set_mode(0o755);
    fs::set_permissions(&path, perms1)?;

    rig::wait();

    assert_eq!(files, files::load_dir(&root1)?);
    assert_eq!(files, files::load_dir(&root2)?);

    let perms2 = fs::metadata(&path)?.permissions();

    // the code currently only sets the user execute bit
    assert_eq!(0o700, perms2.mode() & 0o700);

    return Ok(());
}
