use std::path::{Path, PathBuf};

pub trait StrPath {
    fn as_str(&self) -> &str;
}

impl StrPath for PathBuf {
    fn as_str(&self) -> &str {
        self.to_str().unwrap()
    }
}

// impl StrPath for Option<PathBuf> {
//     fn as_str(&self) -> &str {
//         let path = self.unwrap();
//         path.as_str()
//     }
// }