use crate::common::*;
use ignore::{
    gitignore::{Gitignore, GitignoreBuilder},
    Match, Walk, WalkBuilder,
};
use std::{
    collections::HashSet,
    ffi::OsStr,
    mem,
    path::{Path, PathBuf},
};

pub struct Ignore {
    root: PathBuf,
    files: HashSet<PathBuf>,
    gitignore: Gitignore,
}

impl Ignore {
    pub fn new(root: &Path) -> Self {
        return Self {
            root: PathBuf::from(root),
            files: HashSet::new(),
            gitignore: Gitignore::empty(),
        };
    }

    fn rebuild(&mut self) -> Result<()> {
        let mut builder = GitignoreBuilder::new(&self.root);
        for f in &self.files {
            builder.add(f);
        }
        let gitignore = builder.build()?;
        mem::replace(&mut self.gitignore, gitignore);
        return Ok(());
    }

    pub fn ignore_file_modified(&mut self, file: &Path) -> Result<()> {
        // this is also called if the file changes, so we always have to rebuild
        self.files.insert(PathBuf::from(file));
        return self.rebuild();
    }

    pub fn ignore_file_removed(&mut self, file: &Path) -> Result<()> {
        if self.files.contains(file) {
            self.files.remove(file);
            match self.rebuild() {
                Ok(()) => (),
                Err(err) => {
                    eprintln!("error rebuilding ignore: {}", err);
                    return Err(err);
                }
            }
        }
        return Ok(());
    }

    pub fn is_ignored(&self, path: &Path) -> bool {
        return match self
            .gitignore
            .matched_path_or_any_parents(path, path.is_dir())
        {
            Match::None | Match::Whitelist(_) => false,
            Match::Ignore(_) => true,
        };
    }
}

pub fn is_ignore_file(file: &Path) -> bool {
    return match file.file_name().map(OsStr::to_str).flatten() {
        Some(".gitignore") | Some(".ignore") => true,
        _ => false,
    };
}

pub fn build_walker(root: &Path) -> Walk {
    return WalkBuilder::new(root)
        .hidden(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(true)
        .git_ignore(true)
        .require_git(false)
        .build();
}
