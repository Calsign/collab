use anyhow::{Context, Result};
use context_attribute::context;

#[derive(thiserror::Error, Debug)]
enum CustomError {
    #[error("Error: {0}")]
    Error(String),
}

#[test]
fn basic() {
    #[context("failed for input: {}", _foobar)]
    fn dummy(_foobar: i32) -> Result<()> {
        return Err(CustomError::Error("foobar".to_string()).into());
    }

    assert!(format!("{:?}", dummy(0)).contains("Caused by"));
}

#[test]
fn no_args() {
    #[context("yuck")]
    fn dummy() -> Result<()> {
        return Err(CustomError::Error("whoops".to_string()).into());
    }

    assert!(format!("{:?}", dummy()).contains("Caused by"));
}

#[test]
fn method() {
    struct Foobar {
        x: i32,
    }

    impl Foobar {
        #[context("")]
        fn foobar(&self) -> Result<bool> {
            return if self.x == 0 {
                Ok(true)
            } else {
                Err(CustomError::Error("uh oh".to_string()).into())
            };
        }
    }

    let f = Foobar { x: 1 };
    assert!(format!("{:?}", f.foobar()).contains("Caused by"));
}

#[test]
fn many_args() {
    #[context("foobar")]
    fn foobar(x: i8, y: i8, z: i8) -> Result<()> {
        return Err(CustomError::Error(format!("{}, {}, {}", x, y, z)).into());
    }

    assert!(format!("{:?}", foobar(0, 0, 0)).contains("Caused by"));
}

#[test]
fn associated_fn() {
    #[derive(Debug)]
    struct Foobar {
        x: i32,
    }

    impl Foobar {
        #[context("zero")]
        fn zero() -> Result<Self> {
            let _x = Foobar { x: 0 };
            return Err(CustomError::Error("zero".to_string()).into());
        }
    }

    assert!(format!("{:?}", Foobar::zero()).contains("Caused by"));
}
