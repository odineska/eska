//! Bounded common-picture reads. Archives are inspected in memory, never extracted.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use super::DesignerSource;

mod archive;

const MAX_IMAGE: u64 = 8 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 32 * 1024 * 1024;
const MAX_XML: u64 = 64 * 1024;
const EXTERNAL: &str = "http://v8.1c.ru/8.3/xcf/extrnprops";
const READABLE: &str = "http://v8.1c.ru/8.3/xcf/readable";

/// A browser-displayable image, with its original bytes and a diagnostic filename.
#[derive(Debug)]
pub struct Picture {
    pub mime_type: &'static str,
    pub bytes: Vec<u8>,
    pub file_name: String,
    /// Template variants use the image alpha with the effective text color.
    pub is_template: bool,
    /// One glyph in an atlas, in the selected variant's pixels.
    pub glyph_size: Option<(u32, u32)>,
    pub inputs: Vec<PictureInput>,
}

/// Exact checked payload inputs retained by consumers that watch image resources.
#[derive(Debug)]
pub struct PictureInput {
    pub path: PathBuf,
    pub snapshot: String,
}

/// Preview failures are independent of the metadata properties themselves.
#[derive(Debug)]
pub enum PicturePreview {
    Ready(Picture),
    /// A valid empty picture, including the native export's zero-byte payload.
    Empty {
        inputs: Vec<PictureInput>,
    },
    Failed(PictureError),
}

/// Expected payload failures, distinct from a successfully loaded image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PictureError {
    Missing,
    Unsupported,
    Invalid,
    TooLarge,
    Unavailable,
}

/// Read only the payload explicitly referenced by the external-property descriptor.
pub(super) fn read(source: &DesignerSource, descriptor: &Path) -> PicturePreview {
    resolve(source, descriptor, None, archive::Target::Metadata)
        .unwrap_or_else(PicturePreview::Failed)
}

pub(super) fn read_for_form(
    source: &DesignerSource,
    descriptor: &Path,
    xml: &str,
) -> PicturePreview {
    resolve(source, descriptor, Some(xml), archive::Target::Form)
        .unwrap_or_else(PicturePreview::Failed)
}

/// Resolve all workspace paths through the existing source-containment boundary.
fn resolve(
    source: &DesignerSource,
    descriptor: &Path,
    overlay: Option<&str>,
    target: archive::Target,
) -> Result<PicturePreview, PictureError> {
    let ext = descriptor.with_extension("").join("Ext");
    let mut inputs = Vec::new();
    let xml_path = ext.join("Picture.xml");
    let xml = if let Some(overlay) = overlay {
        if source
            .checked_file(&xml_path)
            .map_err(|_| PictureError::Unavailable)?
            .is_none()
        {
            return Err(PictureError::Missing);
        }
        if overlay.len() as u64 > MAX_XML {
            return Err(PictureError::TooLarge);
        }
        let bytes = overlay.as_bytes().to_vec();
        inputs.push(input(xml_path, &bytes));
        bytes
    } else {
        read_file(source, &xml_path, MAX_XML, &mut inputs)?
    };
    let input = std::str::from_utf8(&xml).map_err(|_| PictureError::Invalid)?;
    let document = roxmltree::Document::parse(input).map_err(|_| PictureError::Invalid)?;
    let root = document.root_element();
    if !root.has_tag_name((EXTERNAL, "ExtPicture")) {
        return Err(PictureError::Invalid);
    }
    let value = root
        .children()
        .find(|node| node.has_tag_name((EXTERNAL, "Picture")))
        .ok_or(PictureError::Missing)?;
    if !value.children().any(|node| node.is_element())
        && value.text().is_none_or(|text| text.trim().is_empty())
    {
        return Ok(PicturePreview::Empty { inputs });
    }
    let name = value
        .children()
        .find(|node| node.has_tag_name((READABLE, "Abs")))
        .and_then(|node| node.text())
        .ok_or(PictureError::Missing)?
        .trim();
    if !safe_name(name) {
        return Err(PictureError::Invalid);
    }
    let bytes = read_file(
        source,
        &ext.join("Picture").join(name),
        MAX_ARCHIVE,
        &mut inputs,
    )?;
    if bytes.is_empty() {
        return Ok(PicturePreview::Empty { inputs });
    }
    let mut picture = if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        archive::picture(&bytes, target)
    } else {
        image(bytes, name.to_owned())
    }?;
    picture.inputs = inputs;
    Ok(PicturePreview::Ready(picture))
}

/// External picture references and manifest variants are local filenames, never paths.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

/// Limit both the declared file length and the actual read to tolerate concurrent changes.
fn read_file(
    source: &DesignerSource,
    relative: &Path,
    limit: u64,
    inputs: &mut Vec<PictureInput>,
) -> Result<Vec<u8>, PictureError> {
    let path = source
        .checked_file(relative)
        .map_err(|_| PictureError::Unavailable)?
        .ok_or(PictureError::Missing)?;
    let file = File::open(path).map_err(|_| PictureError::Unavailable)?;
    if file
        .metadata()
        .map_err(|_| PictureError::Unavailable)?
        .len()
        > limit
    {
        return Err(PictureError::TooLarge);
    }
    let bytes = bounded(file, limit)?;
    inputs.push(input(relative.to_path_buf(), &bytes));
    Ok(bytes)
}

fn input(path: PathBuf, bytes: &[u8]) -> PictureInput {
    PictureInput {
        path,
        snapshot: Sha256::digest(bytes)
            .iter()
            .fold(String::with_capacity(64), |mut text, byte| {
                let _ = write!(text, "{byte:02x}");
                text
            }),
    }
}

/// Bound decompression as well as ordinary reads; never trust a ZIP size alone.
fn bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>, PictureError> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PictureError::Invalid)?;
    if bytes.len() as u64 > limit {
        return Err(PictureError::TooLarge);
    }
    Ok(bytes)
}

/// Detect display formats from contents, not a user-controlled extension.
fn image(bytes: Vec<u8>, file_name: String) -> Result<Picture, PictureError> {
    if bytes.len() as u64 > MAX_IMAGE {
        return Err(PictureError::TooLarge);
    }
    let mime_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.starts_with(b"\0\0\x01\0") {
        "image/x-icon"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        let input = std::str::from_utf8(&bytes).map_err(|_| PictureError::Unsupported)?;
        let document = roxmltree::Document::parse(input).map_err(|_| PictureError::Unsupported)?;
        if !document
            .root_element()
            .has_tag_name(("http://www.w3.org/2000/svg", "svg"))
        {
            return Err(PictureError::Unsupported);
        }
        "image/svg+xml"
    };
    Ok(Picture {
        mime_type,
        bytes,
        file_name,
        is_template: false,
        glyph_size: None,
        inputs: Vec::new(),
    })
}
