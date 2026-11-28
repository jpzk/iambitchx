//! # Media: `/get` and `/upload`, the parts that do not touch Matrix
//!
//! Media messages are numbered per window (`[m.image #3] cat.png`). `/get`
//! picks one of them and saves it under the download directory with a
//! sanitised, never-overwriting file name; `/upload` checks a local file
//! before the Matrix side sends it. Everything here is pure (or plain
//! `std::fs`) so it is tested without a homeserver.
use std::ffi::OsString;
use std::path::PathBuf;

use crate::bitchx::text::is_format;

/// `/upload` refuses files larger than this (100 MB).
pub(crate) const UPLOAD_MAX: u64 = 100 * 1024 * 1024;

/// Media references kept per window for `/get` (oldest dropped first).
pub(crate) const MEDIA_KEEP: usize = 500;

/// Longest file name `/get` writes, in bytes.
const NAME_MAX: usize = 120;

/// A media message as the Matrix side saw it: msgtype (`m.image`), the
/// remote file name (unsanitised, shown only through the line cleaner), and
/// the SDK `MediaSource` as JSON (plain `mxc://` or an encrypted file with
/// its key: the SDK decrypts on download).
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Media {
    pub kind: String,
    pub name: String,
    /// The caption (a body that differs from the file name), if any.
    pub caption: Option<String>,
    pub source: String,
}

/// `Debug` without the source: an encrypted file source carries its key.
impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Media")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("caption", &self.caption)
            .finish_non_exhaustive()
    }
}

impl Media {
    /// The scrollback text of media `#n`: `[m.image #3] cat.png [caption]`.
    pub(crate) fn text(&self, n: usize) -> String {
        match &self.caption {
            Some(c) => format!("[{} #{n}] {} {c}", self.kind, self.name),
            None => format!("[{} #{n}] {}", self.kind, self.name),
        }
    }
}

/// A numbered media message in a window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaRef {
    /// The number shown in the window: `[m.image #n]`.
    pub n: usize,
    pub media: Media,
}

/// Which media `/get` means.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Pick {
    /// `/get [N]`: the Nth-last media of the window (1 = the last).
    Last(usize),
    /// `/get #N`: the media shown as `#N`.
    Number(usize),
}

/// `/get [-o] [N | #N]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GetArgs {
    pub pick: Pick,
    pub open: bool,
}

/// Parse the words after `/get`. The error is the offending word.
pub(crate) fn parse_get(rest: &str) -> Result<GetArgs, String> {
    let mut pick = None;
    let mut open = false;
    for w in rest.split_whitespace() {
        let p = if w == "-o" {
            open = true;
            continue;
        } else if let Some(n) = w.strip_prefix('#') {
            positive(n).map(Pick::Number)
        } else {
            positive(w).map(Pick::Last)
        };
        match (p, pick) {
            (Some(p), None) => pick = Some(p),
            _ => return Err(w.to_string()),
        }
    }
    Ok(GetArgs { pick: pick.unwrap_or(Pick::Last(1)), open })
}

fn positive(s: &str) -> Option<usize> {
    s.parse::<usize>().ok().filter(|n| *n > 0)
}

/// Parse the text after `/upload`: `PATH [caption]`, the path in double
/// quotes if it has spaces. `None` without a path (or with an unclosed
/// quote).
pub(crate) fn parse_upload(rest: &str) -> Option<(String, Option<String>)> {
    let rest = rest.trim();
    let (path, caption) = if let Some(q) = rest.strip_prefix('"') {
        let end = q.find('"')?;
        (&q[..end], &q[end + 1..])
    } else {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        (&rest[..end], &rest[end..])
    };
    if path.is_empty() {
        return None;
    }
    let caption = Some(caption.trim()).filter(|c| !c.is_empty()).map(str::to_string);
    Some((path.to_string(), caption))
}

/// Pick the media `pick` means out of a window's references (oldest first).
pub(crate) fn pick<'a>(refs: &'a [MediaRef], pick: Pick) -> Option<&'a MediaRef> {
    match pick {
        Pick::Last(n) => refs.len().checked_sub(n).map(|i| &refs[i]),
        Pick::Number(n) => refs.iter().find(|r| r.n == n),
    }
}

/// A remote file name made safe to create in the download directory: only
/// the last path component (`/` and `\` both split), no control, bidi or
/// zero-width characters, no leading dots (no `..`, no hidden files), no
/// shell/Windows-hostile punctuation, at most [`NAME_MAX`] bytes with the
/// extension kept. Never empty: `download` if nothing is left.
pub(crate) fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && !is_format(*c) && !matches!(c, '\u{2028}' | '\u{2029}'))
        .map(|c| match c {
            ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned
        .trim_matches(|c: char| c.is_whitespace())
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim();
    let mut out = trimmed.to_string();
    if out.len() > NAME_MAX {
        let (stem, ext) = split_ext(&out);
        let ext = if ext.len() <= 16 { ext } else { "" };
        let mut cut = NAME_MAX - ext.len();
        while !stem.is_char_boundary(cut.min(stem.len())) {
            cut -= 1;
        }
        out = format!("{}{ext}", &stem[..cut.min(stem.len())]);
    }
    if out.is_empty() {
        "download".to_string()
    } else {
        out
    }
}

/// `name.ext` -> (`name`, `.ext`); no extension for dotfiles or names
/// without a dot.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// The k-th candidate for `name`: `name` itself, then `stem-1.ext`,
/// `stem-2.ext`, ...
pub(crate) fn numbered(name: &str, k: usize) -> String {
    if k == 0 {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    format!("{stem}-{k}{ext}")
}

/// Where `/get` saves: `$XDG_DOWNLOAD_DIR` if set to an absolute path, else
/// `~/Downloads/iambitchx/`.
pub(crate) fn download_dir(xdg: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match xdg.map(PathBuf::from).filter(|p| p.is_absolute()) {
        Some(p) => Some(p),
        None => home.map(|h| h.join("Downloads").join("iambitchx")),
    }
}

/// The content type for an upload, from the file extension (`/upload`
/// sends `image/*` as `m.image`, everything else as a file/audio/video by
/// the SDK's mapping).
pub(crate) fn mime_of(name: &str) -> &'static str {
    let ext = split_ext(name).1.trim_start_matches('.').to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "heic" => "image/heic",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "json" => "application/json",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",
        _ => "application/octet-stream",
    }
}

/// `/get -o` does not hand these to the desktop opener: a remote sender
/// picks the name, and these types run code (scripts, launchers,
/// installers) or script-capable documents (HTML, SVG) when opened.
pub(crate) fn risky_to_open(name: &str) -> bool {
    let ext = split_ext(name).1.trim_start_matches('.').to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "" | "desktop"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "csh"
            | "exe"
            | "bat"
            | "cmd"
            | "com"
            | "msi"
            | "scr"
            | "lnk"
            | "ps1"
            | "vbs"
            | "jar"
            | "appimage"
            | "run"
            | "bin"
            | "command"
            | "app"
            | "py"
            | "pl"
            | "rb"
            | "js"
            | "deb"
            | "rpm"
            | "html"
            | "htm"
            | "xhtml"
            | "svg"
    )
}

/// Human-readable size: `512 B`, `3.4 KB`, `12.0 MB`.
pub(crate) fn human_size(n: u64) -> String {
    const K: u64 = 1024;
    let tenths = |unit: u64| {
        let t = n * 10 / unit;
        format!("{}.{}", t / 10, t % 10)
    };
    if n < K {
        format!("{n} B")
    } else if n < K * K {
        format!("{} KB", tenths(K))
    } else {
        format!("{} MB", tenths(K * K))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn media(n: usize) -> MediaRef {
        MediaRef {
            n,
            media: Media {
                kind: "m.image".into(),
                name: format!("{n}.png"),
                caption: None,
                source: String::new(),
            },
        }
    }

    #[test]
    fn test_sanitize_strips_path_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("/etc/shadow"), "shadow");
        assert_eq!(sanitize_filename("..\\..\\windows\\system32\\x.dll"), "x.dll");
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename("."), "download");
        assert_eq!(sanitize_filename("a/.."), "download");
        assert_eq!(sanitize_filename("foo/"), "download");
        assert_eq!(sanitize_filename(""), "download");
        assert_eq!(sanitize_filename("...hidden"), "hidden");
        assert_eq!(sanitize_filename(".bashrc"), "bashrc");
        for evil in ["../x", "a/../../b", "..\\x", "/abs", "x/./../y"] {
            let s = sanitize_filename(evil);
            assert!(!s.contains('/') && !s.contains('\\'), "{evil} -> {s}");
            assert!(!s.starts_with('.'), "{evil} -> {s}");
            assert_eq!(Path::new(&s).components().count(), 1, "{evil} -> {s}");
        }
    }

    #[test]
    fn test_sanitize_strips_controls_and_bidi() {
        assert_eq!(sanitize_filename("a\u{1b}[31mb.png"), "a[31mb.png");
        assert_eq!(sanitize_filename("cat\u{202e}gnp.exe"), "catgnp.exe");
        assert_eq!(sanitize_filename("x\0y\n.txt"), "xy.txt");
        assert_eq!(sanitize_filename("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize_filename("  cat pic.png  "), "cat pic.png");
        assert_eq!(sanitize_filename("trailing. . "), "trailing");
    }

    #[test]
    fn test_sanitize_caps_length_keeping_extension() {
        let long = format!("{}.png", "é".repeat(200));
        let s = sanitize_filename(&long);
        assert!(s.len() <= NAME_MAX, "{}", s.len());
        assert!(s.ends_with(".png"));
    }

    #[test]
    fn test_numbered_names() {
        assert_eq!(numbered("cat.png", 0), "cat.png");
        assert_eq!(numbered("cat.png", 1), "cat-1.png");
        assert_eq!(numbered("cat.png", 2), "cat-2.png");
        assert_eq!(numbered("README", 3), "README-3");
        assert_eq!(numbered("a.tar.gz", 1), "a.tar-1.gz");
    }

    #[test]
    fn test_download_dir() {
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            download_dir(None, home.clone()),
            Some(PathBuf::from("/home/u/Downloads/iambitchx"))
        );
        assert_eq!(
            download_dir(Some("/data/dl".into()), home.clone()),
            Some(PathBuf::from("/data/dl"))
        );
        // relative / empty XDG values are ignored (XDG spec)
        assert_eq!(
            download_dir(Some("rel".into()), home.clone()),
            Some(PathBuf::from("/home/u/Downloads/iambitchx"))
        );
        assert_eq!(
            download_dir(Some("".into()), home),
            Some(PathBuf::from("/home/u/Downloads/iambitchx"))
        );
        assert_eq!(download_dir(None, None), None);
    }

    #[test]
    fn test_parse_get() {
        let g = |s: &str| parse_get(s);
        assert_eq!(g(""), Ok(GetArgs { pick: Pick::Last(1), open: false }));
        assert_eq!(g("3"), Ok(GetArgs { pick: Pick::Last(3), open: false }));
        assert_eq!(g("-o"), Ok(GetArgs { pick: Pick::Last(1), open: true }));
        assert_eq!(g("-o 2"), Ok(GetArgs { pick: Pick::Last(2), open: true }));
        assert_eq!(g("2 -o"), Ok(GetArgs { pick: Pick::Last(2), open: true }));
        assert_eq!(g("#7"), Ok(GetArgs { pick: Pick::Number(7), open: false }));
        assert_eq!(g("-o #7"), Ok(GetArgs { pick: Pick::Number(7), open: true }));
        assert_eq!(g("0"), Err("0".into()));
        assert_eq!(g("#0"), Err("#0".into()));
        assert_eq!(g("-1"), Err("-1".into()));
        assert_eq!(g("x"), Err("x".into()));
        assert_eq!(g("1 2"), Err("2".into()));
        assert_eq!(g("-x"), Err("-x".into()));
    }

    #[test]
    fn test_parse_upload() {
        let u = |s: &str| parse_upload(s);
        assert_eq!(u("/tmp/a.png"), Some(("/tmp/a.png".into(), None)));
        assert_eq!(
            u("  ~/a.png  look at this "),
            Some(("~/a.png".into(), Some("look at this".into())))
        );
        assert_eq!(u("\"/tmp/my cat.png\""), Some(("/tmp/my cat.png".into(), None)));
        assert_eq!(
            u("\"/tmp/my cat.png\" meow"),
            Some(("/tmp/my cat.png".into(), Some("meow".into())))
        );
        assert_eq!(u(""), None);
        assert_eq!(u("   "), None);
        assert_eq!(u("\"unclosed"), None);
        assert_eq!(u("\"\" caption"), None);
    }

    #[test]
    fn test_pick() {
        let refs = vec![media(1), media(2), media(3)];
        assert_eq!(pick(&refs, Pick::Last(1)).map(|r| r.n), Some(3));
        assert_eq!(pick(&refs, Pick::Last(3)).map(|r| r.n), Some(1));
        assert_eq!(pick(&refs, Pick::Last(4)), None);
        assert_eq!(pick(&refs, Pick::Number(2)).map(|r| r.n), Some(2));
        assert_eq!(pick(&refs, Pick::Number(9)), None);
        assert_eq!(pick(&[], Pick::Last(1)), None);
    }

    #[test]
    fn test_mime_of() {
        assert_eq!(mime_of("a.PNG"), "image/png");
        assert_eq!(mime_of("x.jpeg"), "image/jpeg");
        assert_eq!(mime_of("song.mp3"), "audio/mpeg");
        assert_eq!(mime_of("doc.pdf"), "application/pdf");
        assert_eq!(mime_of("noext"), "application/octet-stream");
        assert_eq!(mime_of(".png"), "application/octet-stream");
    }

    #[test]
    fn test_risky_to_open() {
        for r in [
            "x.desktop",
            "a.SH",
            "setup.exe",
            "page.html",
            "pic.svg",
            "README",
        ] {
            assert!(risky_to_open(r), "{r}");
        }
        for ok in ["cat.png", "a.pdf", "song.mp3", "notes.txt"] {
            assert!(!risky_to_open(ok), "{ok}");
        }
    }

    #[test]
    fn test_human_size() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }
}
