//! # `/get` and `/upload` on the Matrix side
//!
//! Downloads go through the SDK media API (`get_media_content`), which
//! fetches `mxc://` content and decrypts encrypted attachments with the key
//! carried in the event; uploads go through `Room::send_attachment`, which
//! encrypts in encrypted rooms. File names and limits come from the pure
//! helpers in [`crate::bitchx::media`]; this file adds the filesystem.
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use matrix_sdk::Client;
use matrix_sdk::attachment::AttachmentConfig;
use matrix_sdk::media::{MediaFormat, MediaRequestParameters};
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::ruma::events::room::message::TextMessageEventContent;

use crate::bitchx::media::{
    Media, UPLOAD_MAX, download_dir, human_size, mime_of, numbered, risky_to_open,
    sanitize_filename,
};
use crate::bitchx::model::{Line, MxEvent};
use crate::bitchx::text::terminal_text;
use crate::matrix::Tx;

/// Create a new file for `name` in `dir`, never touching an existing one:
/// `O_CREAT|O_EXCL` (`create_new`), so neither an existing file nor a
/// planted symlink is followed or overwritten; on a clash the next
/// [`numbered`] name (`-1`, `-2`, ...) is tried.
pub(crate) fn create_unique(dir: &Path, name: &str) -> io::Result<(PathBuf, File)> {
    for k in 0..10_000 {
        let path = dir.join(numbered(name, k));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {},
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other(format!("too many files named {name}")))
}

/// Check a local file for `/upload`: it must exist, be a regular file (not
/// a directory) and be at most [`UPLOAD_MAX`]. Returns its size.
pub(crate) fn check_upload(path: &Path) -> Result<u64, String> {
    let shown = terminal_text(&path.display().to_string());
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(format!("No such file: {shown}"));
        },
        Err(e) => return Err(format!("Cannot read {shown}: {e}")),
    };
    if meta.is_dir() {
        return Err(format!("{shown} is a directory, not a file"));
    }
    if !meta.is_file() {
        return Err(format!("{shown} is not a regular file"));
    }
    if meta.len() > UPLOAD_MAX {
        return Err(format!(
            "{shown} is too large ({}, limit {})",
            human_size(meta.len()),
            human_size(UPLOAD_MAX)
        ));
    }
    Ok(meta.len())
}

/// Write `data` to a fresh file for `name` in `dir` (created if needed);
/// a failed write removes the partial file.
fn save(dir: &Path, name: &str, data: &[u8]) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let (path, mut file) = create_unique(dir, &sanitize_filename(name))?;
    if let Err(e) = file.write_all(data).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

fn info(tx: &Tx, room_id: &OwnedRoomId, line: Line) {
    let _ = tx.send(MxEvent::Info { room_id: Some(room_id.clone()), line });
}

/// `/get`: download media `#n`, save it, print the path; open it if asked
/// (and the type is not one that runs code).
pub(crate) async fn get(
    client: &Client,
    room_id: OwnedRoomId,
    n: usize,
    media: Media,
    open: bool,
    tx: &Tx,
) -> anyhow::Result<()> {
    let source: MediaSource = serde_json::from_str(&media.source)?;
    let request = MediaRequestParameters { source, format: MediaFormat::File };
    let data = client.media().get_media_content(&request, true).await?;
    let dir = download_dir(std::env::var_os("XDG_DOWNLOAD_DIR"), dirs::home_dir())
        .ok_or_else(|| anyhow::anyhow!("no download directory (set XDG_DOWNLOAD_DIR)"))?;
    let path = save(&dir, &media.name, &data)?;
    let shown = terminal_text(&path.display().to_string());
    let size = human_size(data.len() as u64);
    info(tx, &room_id, Line::Info(format!("Saved [{} #{n}] to {shown} ({size})", media.kind)));
    if open {
        if risky_to_open(&path.to_string_lossy()) {
            let line = Line::Error(format!("Not opening {shown}: file type can run code"));
            info(tx, &room_id, line);
        } else {
            let p = path.clone();
            match tokio::task::spawn_blocking(move || open::that_detached(p)).await? {
                Ok(()) => info(tx, &room_id, Line::Info(format!("Opening {shown}"))),
                Err(e) => info(tx, &room_id, Line::Error(format!("Cannot open {shown}: {e}"))),
            }
        }
    }
    Ok(())
}

/// `/upload`: send a local file; the SDK picks `m.image` for `image/*`,
/// `m.audio`/`m.video` for those, `m.file` otherwise.
pub(crate) async fn upload(
    client: &Client,
    room_id: OwnedRoomId,
    path: String,
    caption: Option<String>,
    tx: &Tx,
) -> anyhow::Result<()> {
    let path = PathBuf::from(shellexpand::tilde(&path).as_ref());
    if let Err(msg) = check_upload(&path) {
        info(tx, &room_id, Line::Error(msg));
        return Ok(());
    }
    let data = std::fs::read(&path)?;
    if data.len() as u64 > UPLOAD_MAX {
        anyhow::bail!("file grew past {} while reading", human_size(UPLOAD_MAX));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow::anyhow!("no file name"))?;
    let mime = mime_of(&name);
    let content_type = mime.parse().map_err(|e| anyhow::anyhow!("bad mime {mime}: {e}"))?;
    let room = client
        .get_room(&room_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown channel {room_id}"))?;
    let shown = terminal_text(&name);
    let size = human_size(data.len() as u64);
    info(tx, &room_id, Line::Info(format!("Uploading {shown} ({size}, {mime})")));
    let config = AttachmentConfig::new().caption(caption.map(TextMessageEventContent::plain));
    room.send_attachment(name, &content_type, data, config).await?;
    info(tx, &room_id, Line::Info(format!("Uploaded {shown}")));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("iambitchx-media-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_create_unique_never_overwrites() {
        let d = tmpdir("unique");
        std::fs::write(d.join("cat.png"), b"old").unwrap();
        let (p1, _) = create_unique(&d, "cat.png").unwrap();
        assert_eq!(p1, d.join("cat-1.png"));
        let (p2, _) = create_unique(&d, "cat.png").unwrap();
        assert_eq!(p2, d.join("cat-2.png"));
        assert_eq!(std::fs::read(d.join("cat.png")).unwrap(), b"old");
        let (p3, _) = create_unique(&d, "new.txt").unwrap();
        assert_eq!(p3, d.join("new.txt"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_create_unique_does_not_follow_symlinks() {
        let d = tmpdir("symlink");
        let target = d.join("target");
        std::fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, d.join("x.png")).unwrap();
        let (p, _) = create_unique(&d, "x.png").unwrap();
        assert_eq!(p, d.join("x-1.png"));
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn test_save_traversal_stays_in_dir() {
        let d = tmpdir("save");
        let inner = d.join("dl");
        for evil in [
            "../escape.txt",
            "../../escape.txt",
            "/tmp/escape.txt",
            "..\\escape.txt",
        ] {
            let p = save(&inner, evil, b"x").unwrap();
            assert_eq!(p.parent(), Some(inner.as_path()), "{evil} -> {}", p.display());
        }
        assert!(!d.join("escape.txt").exists());
        // the same name twice: -1, -2, the first never overwritten
        let names: Vec<_> = std::fs::read_dir(&inner)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        let mut names = names;
        names.sort();
        assert_eq!(names, ["escape-1.txt", "escape-2.txt", "escape-3.txt", "escape.txt"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn test_check_upload() {
        let d = tmpdir("upload");
        let f = d.join("ok.txt");
        std::fs::write(&f, b"hello").unwrap();
        assert_eq!(check_upload(&f), Ok(5));
        assert!(check_upload(&d).unwrap_err().contains("is a directory"));
        assert!(check_upload(&d.join("missing")).unwrap_err().starts_with("No such file"));
        let big = d.join("big.bin");
        let fh = File::create(&big).unwrap();
        fh.set_len(UPLOAD_MAX + 1).unwrap(); // sparse: no disk used
        assert!(check_upload(&big).unwrap_err().contains("too large"));
        fh.set_len(UPLOAD_MAX).unwrap();
        assert_eq!(check_upload(&big), Ok(UPLOAD_MAX));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
