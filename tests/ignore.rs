use std::fs;

use test_common::{common::Result, dir, file, files, path, rig};

#[test]
fn ignorefile() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let ignore_file = file!(
        r#"
ignored_file
ignored_directory
*.ignored
!whitelisted.ignored
"#
    );

    let files1 = dir! {
        ".ignore" => ignore_file.clone(),
        "ignored_file" => file!(""),
        "ignored_directory" => dir! {
            "a" => file!(""),
            "b" => file!("")
        },
        "a.ignored" => file!(""),
        "b.ignored" => file!(""),
        "whitelisted.ignored" => file!("")
    };

    let files2 = dir! {
        ".ignore" => ignore_file,
        "whitelisted.ignored" => file!("")
    };

    files1.apply(&root1)?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    rig::wait();

    assert_eq!(files1, files::load_dir(&root1)?);
    assert_eq!(files2, files::load_dir(&root2)?);

    return Ok(());
}

#[test]
fn ignorefile_item_removed() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files1 = dir! {
        ".ignore" => file!("foobar"),
        "foobar" => file!("")
    };

    files1.apply(&root1)?;

    rig::wait();

    let files2 = dir! {
        ".ignore" => file!("foobar")
    };

    assert_eq!(files1, files::load_dir(&root1)?);
    assert_eq!(files2, files::load_dir(&root2)?);

    let files3 = dir! {
        ".ignore" => file!(""),
        "foobar" => file!("")
    };

    files3.apply(&root1)?;

    rig::wait();

    assert_eq!(files3, files::load_dir(&root1)?);
    assert_eq!(files3, files::load_dir(&root2)?);

    return Ok(());
}

#[test]
fn ignorefile_item_added() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files1 = dir! {
        ".ignore" => file!(""),
        "foobar" => file!("")
    };

    files1.apply(&root1)?;

    rig::wait();

    let files2 = dir! {
        ".ignore" => file!(""),
        "foobar" => file!("")
    };

    assert_eq!(files1, files::load_dir(&root1)?);
    assert_eq!(files2, files::load_dir(&root2)?);

    let files3 = dir! {
        ".ignore" => file!("foobar"),
        "foobar" => file!("")
    };

    files3.apply(&root1)?;

    rig::wait();

    assert_eq!(files3, files::load_dir(&root1)?);
    assert_eq!(files3, files::load_dir(&root2)?);

    let files4 = dir! {
        ".ignore" => file!("foobar"),
        "foobar" => file!("some ignored text")
    };

    files4.apply(&root1)?;

    rig::wait();

    assert_eq!(files4, files::load_dir(&root1)?);
    assert_eq!(files3, files::load_dir(&root2)?);

    return Ok(());
}

#[test]
fn ignorefile_deleted() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files1 = dir! {
        ".ignore" => file!("foobar"),
        "foobar" => file!("")
    };

    files1.apply(&root1)?;

    rig::wait();

    let files2 = dir! {
        ".ignore" => file!("foobar")
    };

    assert_eq!(files1, files::load_dir(&root1)?);
    assert_eq!(files2, files::load_dir(&root2)?);

    fs::remove_file(path!(&root1, ".ignore"))?;

    rig::wait();

    let files3 = dir! {
        "foobar" => file!("")
    };

    assert_eq!(files3, files::load_dir(&root1)?);
    assert_eq!(files3, files::load_dir(&root2)?);

    return Ok(());
}

#[test]
fn ignorefile_created() -> Result<()> {
    let root1 = rig::tempdir()?;
    let root2 = rig::tempdir()?;

    let daemon1 = rig::daemon("r1", &root1)?;
    let _daemon2 = rig::connect("r2", &root2, &daemon1)?;

    let files1 = dir! {
        "foobar" => file!("")
    };

    files1.apply(&root1)?;

    rig::wait();

    assert_eq!(files1, files::load_dir(&root1)?);
    assert_eq!(files1, files::load_dir(&root2)?);

    let files2 = dir! {
        ".ignore" => file!("foobar"),
        "foobar" => file!("")
    };

    files2.apply(&root1)?;

    rig::wait();

    assert_eq!(files2, files::load_dir(&root1)?);
    assert_eq!(files2, files::load_dir(&root2)?);

    let files3 = dir! {
        ".ignore" => file!("foobar"),
        "foobar" => file!("some ignored text")
    };

    files3.apply(&root1)?;

    rig::wait();

    assert_eq!(files3, files::load_dir(&root1)?);
    assert_eq!(files2, files::load_dir(&root2)?);

    return Ok(());
}
