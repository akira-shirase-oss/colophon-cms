//! Where `colophon login` keeps its tokens: `credentials.json` in `$COLOPHON_CONFIG_DIR`, else
//! `$XDG_CONFIG_HOME/colophon`, else `~/.config/colophon`. One profile per server and person,
//! `default` unless `--profile` says otherwise. The file is created readable by its owner only.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Exit, client::Failure};

/// One signed-in profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Profile {
    pub(crate) server: String,
    pub(crate) token: String,
    pub(crate) expires_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    profiles: BTreeMap<String, Profile>,
}

fn directory() -> Result<PathBuf, Failure> {
    let env = |key: &str| {
        std::env::var_os(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    env("COLOPHON_CONFIG_DIR")
        .or_else(|| env("XDG_CONFIG_HOME").map(|dir| dir.join("colophon")))
        .or_else(|| env("HOME").map(|home| home.join(".config/colophon")))
        .ok_or_else(|| {
            Failure::new(
                Exit::Usage,
                "no place for credentials: set COLOPHON_CONFIG_DIR or HOME",
            )
        })
}

fn path() -> Result<PathBuf, Failure> {
    Ok(directory()?.join("credentials.json"))
}

async fn read() -> Result<File, Failure> {
    match tokio::fs::read(path()?).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            Failure::new(
                Exit::Usage,
                format!("the credentials file is not valid: {error}"),
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
        Err(error) => Err(Failure::new(
            Exit::Internal,
            format!("could not read the credentials file: {error}"),
        )),
    }
}

async fn write(file: &File) -> Result<(), Failure> {
    write_to(&path()?, file).await
}

/// Writes the credentials to `path`, readable by its owner only: created mode 600, and an existing
/// file with a wider mode (copied in, or made by an older build) is narrowed to 600 before a
/// byte of the new tokens is written into it.
async fn write_to(path: &std::path::Path, file: &File) -> Result<(), Failure> {
    let internal = |error: std::io::Error| {
        Failure::new(
            Exit::Internal,
            format!("could not write {}: {error}", path.display()),
        )
    };
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(internal)?;
    }
    let bytes = serde_json::to_vec_pretty(file)
        .map_err(|error| Failure::new(Exit::Internal, error.to_string()))?;
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut handle = options.open(path).await.map_err(internal)?;
    // `mode` above applies only when the file is created.
    #[cfg(unix)]
    handle
        .set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .await
        .map_err(internal)?;
    tokio::io::AsyncWriteExt::write_all(&mut handle, &bytes)
        .await
        .map_err(internal)?;
    tokio::io::AsyncWriteExt::flush(&mut handle)
        .await
        .map_err(internal)
}

/// The profile named `name`, or a hint to sign in.
pub(crate) async fn profile(name: &str) -> Result<Profile, Failure> {
    read().await?.profiles.remove(name).ok_or_else(|| {
        Failure::new(
            Exit::Denied,
            format!("not signed in (profile {name:?}); run `colophon login`"),
        )
    })
}

pub(crate) async fn save(name: &str, profile: Profile) -> Result<(), Failure> {
    let mut file = read().await?;
    file.profiles.insert(name.to_owned(), profile);
    write(&file).await
}

pub(crate) async fn forget(name: &str) -> Result<(), Failure> {
    let mut file = read().await?;
    file.profiles.remove(name);
    write(&file).await
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::{File, write_to};

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path)
            .expect("invariant: the test just wrote this file")
            .permissions()
            .mode()
            & 0o777
    }

    /// A credentials file that already exists with a wider mode is narrowed to 600 on the next
    /// write, so a new token never lands in a file others can read.
    #[tokio::test]
    async fn an_existing_wider_file_is_narrowed_to_owner_only() {
        let dir = std::env::temp_dir().join(format!("colophon-credentials-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("invariant: the temp dir is writable");
        let path = dir.join("credentials.json");
        std::fs::write(&path, b"{}").expect("invariant: the temp dir is writable");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("invariant: we own the file");

        write_to(&path, &File::default()).await.expect("the write succeeds");
        assert_eq!(mode(&path), 0o600, "an existing 644 file is narrowed to 600");

        std::fs::remove_file(&path).expect("invariant: we own the file");
        write_to(&path, &File::default()).await.expect("the write succeeds");
        assert_eq!(mode(&path), 0o600, "a new file is created 600");
        std::fs::remove_dir_all(&dir).expect("invariant: we own the dir");
    }
}
