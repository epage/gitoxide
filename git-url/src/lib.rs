#![forbid(unsafe_code)]

#[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone, Copy)]
#[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
pub enum Protocol {
    File,
    Git,
    Ssh,
    Http,
    Https,
}

pub mod owned {
    use crate::Protocol;
    use bstr::ByteSlice;
    use std::{
        borrow::Cow,
        path::{Path, PathBuf},
    };

    #[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone)]
    #[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
    pub enum UserExpansion {
        Current,
        Name(String),
    }

    #[derive(PartialEq, Eq, Debug, Hash, Ord, PartialOrd, Clone)]
    #[cfg_attr(feature = "serde1", derive(serde::Serialize, serde::Deserialize))]
    pub struct Url {
        pub protocol: Protocol,
        pub user: Option<String>,
        pub host: Option<String>,
        pub port: Option<u16>,
        pub path: Vec<u8>,
        pub expand_user: Option<UserExpansion>,
    }

    impl Default for Url {
        fn default() -> Self {
            Url {
                protocol: Protocol::Ssh,
                user: None,
                host: None,
                port: None,
                path: Vec::new(),
                expand_user: None,
            }
        }
    }

    impl Url {
        pub fn expand_path_with(
            &self,
            home_for_user: impl FnOnce(&UserExpansion) -> Option<PathBuf>,
        ) -> Option<PathBuf> {
            fn make_relative(path: &Path) -> Cow<Path> {
                if path.is_relative() {
                    return path.into();
                }
                path.components().skip(1).collect::<PathBuf>().into()
            }
            match self.expand_user.as_ref() {
                Some(user) => home_for_user(user)
                    .and_then(|base| self.path.to_path().ok().map(|path| base.join(make_relative(path)))),
                None => self.path.to_path().ok().map(ToOwned::to_owned),
            }
        }
    }
}

#[doc(inline)]
pub use owned::Url as Owned;

pub mod parse;
#[doc(inline)]
pub use parse::parse;
