use assert_cmd::Command;
use std::thread::sleep;
use std::time::Duration;
use tempdir::TempDir;

use test_common::{dir, file, file_node, rig};

#[test]
fn basic() {
    let root1 = TempDir::new("collab_test").unwrap();
    let root2 = TempDir::new("collab_test").unwrap();

    let files = dir! {
        "foobar" => file!("x"),
        "empty_dir" => dir! {}
    };

    files.apply(root1.path()).unwrap();

    let daemon1 = rig::daemon("r1", &root1).unwrap();
    let daemon2 = rig::connect("r2", &root2, &daemon1).unwrap();

    sleep(Duration::from_millis(100));

    assert_eq!(
        file_node::load_dir(root1.path()).unwrap(),
        file_node::load_dir(root2.path()).unwrap(),
    );
}
