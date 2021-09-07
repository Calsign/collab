use relative_path::{RelativePath, RelativePathBuf};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::common;

// TODO: add support for symlinks?

#[derive(PartialEq, Eq, Debug)]
pub enum Node<T> {
    Dir(HashMap<String, Node<T>>),
    File(T),
}

#[derive(Hash, PartialEq, Eq, Debug)]
pub struct File {
    pub data: String,
}

#[macro_export]
macro_rules! dir(
    { $($name:expr => $contents:expr),+ } => {
        {
            let mut map = std::collections::HashMap::new();
            $(
                map.insert($name.to_string(), $contents);
            )+
            crate::files::Node::Dir(map)
        }
    };
    { } => {
        {
            crate::files::Node::Dir(std::collections::HashMap::new())
        }
    }
);

#[macro_export]
macro_rules! file(
    { $contents:expr } => {
        {
            crate::files::Node::File(crate::files::File {
                data: $contents.to_string()
            })
        }
    }
);

#[derive(Hash, PartialEq, Eq, Debug)]
pub enum PathNode {
    Dir(RelativePathBuf),
    File(RelativePathBuf, String),
}

impl Node<File> {
    pub fn apply<P: AsRef<Path>>(&self, root: &P) -> common::Result<()> {
        match &self {
            Node::Dir(files) => {
                fs::create_dir_all(root)?;
                for (name, node) in files {
                    let mut path = root.as_ref().to_path_buf();
                    path.push(name);
                    node.apply(&path)?;
                }
                return Ok(());
            }
            Node::File(contents) => Ok(fs::write(root, &contents.data.as_bytes())?),
        }
    }

    fn add_files(&self, path: &RelativePath, set: &mut HashSet<PathNode>) -> common::Result<()> {
        match &self {
            Node::Dir(files) => {
                let mut inner_path = path.to_relative_path_buf();
                set.insert(PathNode::Dir(inner_path.clone()));
                for (name, node) in files {
                    inner_path.push(name);
                    node.add_files(&inner_path, set)?;
                }
                return Ok(());
            }
            Node::File(contents) => {
                set.insert(PathNode::File(
                    path.to_relative_path_buf(),
                    contents.data.clone(),
                ));
                return Ok(());
            }
        }
    }

    pub fn files(&self) -> common::Result<HashSet<PathNode>> {
        let mut set = HashSet::new();
        self.add_files(RelativePath::new(""), &mut set)?;
        return Ok(set);
    }
}

pub fn load_dir<P: AsRef<Path>>(root: &P) -> common::Result<Node<File>> {
    let root = root.as_ref();
    let metadata = fs::metadata(root)?;
    return Ok(if metadata.is_dir() {
        let mut contents = HashMap::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;

            let name = entry.file_name().into_string().unwrap();
            let mut path = root.to_path_buf();
            path.push(entry.file_name());

            let child = load_dir(&path)?;
            contents.insert(name, child);
        }
        Node::Dir(contents)
    } else if metadata.is_file() {
        let data = fs::read_to_string(root)?;
        Node::File(File { data })
    } else {
        panic!("invalid case - neither directory nor file");
    });
}
