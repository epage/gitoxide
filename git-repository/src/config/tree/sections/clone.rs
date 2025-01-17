use crate::{
    config,
    config::tree::{keys, Clone, Key, Section},
};

impl Clone {
    /// The `clone.defaultRemoteName` key.
    pub const DEFAULT_REMOTE_NAME: keys::RemoteName =
        keys::RemoteName::new_remote_name("defaultRemoteName", &config::Tree::CLONE);
}

impl Section for Clone {
    fn name(&self) -> &str {
        "clone"
    }

    fn keys(&self) -> &[&dyn Key] {
        &[&Self::DEFAULT_REMOTE_NAME]
    }
}
