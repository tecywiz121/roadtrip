use futures::pin_mut;

use roadtrip_walkdir::WalkDir;

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use tokio::stream::StreamExt;

#[tokio::test]
async fn tree() -> Result<(), String> {
    let manifest_dir =
        env::var_os("CARGO_MANIFEST_DIR").ok_or("no manifest dir")?;
    let mut root = PathBuf::from(manifest_dir);
    root.push("tests");
    root.push("testdata");

    let walkdir = WalkDir::new(root.clone()).walk();
    pin_mut!(walkdir);

    let mut expected = HashMap::new();
    expected.insert(Path::new(""), true);
    expected.insert(Path::new("dir0"), true);
    expected.insert(Path::new("dir1"), true);
    expected.insert(Path::new("dir2"), true);
    expected.insert(Path::new("dir2/dir3"), true);
    expected.insert(Path::new("dir2/dir4"), true);
    expected.insert(Path::new("dir2/dir4/dir5"), true);

    expected.insert(Path::new("file0"), false);
    expected.insert(Path::new("dir0/file3"), false);
    expected.insert(Path::new("dir1/file1"), false);
    expected.insert(Path::new("dir2/dir3/file2"), false);
    expected.insert(Path::new("dir2/dir4/dir5/file4"), false);

    while let Some(Ok(entry)) = walkdir.next().await {
        let stripped = entry
            .path()
            .strip_prefix(&root)
            .map_err(|x| x.to_string())?
            .to_owned();

        let is_dir = expected.remove(stripped.as_path()).ok_or("extra path")?;

        if entry.file_type().is_dir() != is_dir {
            return Err(format!("{:?} incorrect type", stripped));
        }
    }

    if expected.is_empty() {
        Ok(())
    } else {
        Err("missing path(s)".into())
    }
}
