use test_common::{common::Result, dir, file, files, rig};

#[test]
fn connect() -> Result<()> {
    let root = rig::tempdir()?;
    let daemon = rig::daemon("r1", &root)?;

    let files = dir! {
        "file" => file!("foobar")
    };
    files.apply(&root)?;

    rig::wait();

    let attach = rig::attach(&daemon, "file")?;

    rig::wait();

    return Ok(());
}

#[test]
fn basic_send() -> Result<()> {
    let root1 = rig::tempdir()?;
    let daemon1 = rig::daemon("r1", &root1)?;

    let root2 = rig::tempdir()?;
    let daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files = dir! {
        "file" => file!("")
    };
    files.apply(&root1)?;

    rig::wait();

    let mut attach1 = rig::attach(&daemon1, "file")?;
    let mut attach2 = rig::attach(&daemon2, "file")?;

    rig::wait();

    let diff = rig::BufferDiff::new(0, 0, "x");

    attach1.send_diff(&diff)?;

    rig::wait();

    assert_eq!(attach2.pop_diff()?, Some(diff));

    assert_eq!(attach1.pop_diff()?, None);
    assert_eq!(attach2.pop_diff()?, None);

    return Ok(());
}
