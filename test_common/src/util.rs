use std::env;
use std::path;

// This code is taken from assert_cmd. It wasn't exposed publicly, so here we are.

// Adapted from
// https://github.com/rust-lang/cargo/blob/485670b3983b52289a2f353d589c57fae2f60f82/tests/testsuite/support/mod.rs#L507
fn target_dir() -> path::PathBuf {
    env::current_exe()
        .ok()
        .map(|mut path| {
            path.pop();
            if path.ends_with("deps") {
                path.pop();
            }
            path
        })
        .unwrap()
}

fn cargo_bin_str(name: &str) -> path::PathBuf {
    let env_var = format!("CARGO_BIN_EXE_{}", name);
    std::env::var_os(&env_var)
        .map(|p| p.into())
        .unwrap_or_else(|| target_dir().join(format!("{}{}", name, env::consts::EXE_SUFFIX)))
}

/// Look up the path to a cargo-built binary within an integration test.
pub fn cargo_bin<S: AsRef<str>>(name: S) -> path::PathBuf {
    cargo_bin_str(name.as_ref())
}
