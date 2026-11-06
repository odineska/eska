use std::{io::Cursor, path::Path};

use super::{MAX_IMAGE, MAX_XML, Picture, PictureError, bounded, image, safe_name};

const MAX_ENTRIES: usize = 512;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Metadata,
    Form,
}

pub(super) fn picture(bytes: &[u8], target: Target) -> Result<Picture, PictureError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| PictureError::Invalid)?;
    if archive.len() > MAX_ENTRIES {
        return Err(PictureError::TooLarge);
    }
    let declared = match archive.by_name("manifest.xml") {
        Ok(file) => {
            if file.size() > MAX_XML {
                return Err(PictureError::TooLarge);
            }
            Some(variant(&bounded(file, MAX_XML)?, target)?)
        }
        Err(zip::result::ZipError::FileNotFound) => None,
        Err(_) => return Err(PictureError::Invalid),
    };
    // Filename discovery is only for archives without a manifest, never for broken references.
    let (name, is_template) = match declared {
        Some(variant) => variant,
        None => (
            fallback_name(&archive)?.ok_or(PictureError::Unsupported)?,
            false,
        ),
    };
    let file = archive.by_name(&name).map_err(|_| PictureError::Invalid)?;
    if file.size() > MAX_IMAGE {
        return Err(PictureError::TooLarge);
    }
    let mut picture = image(bounded(file, MAX_IMAGE)?, name)?;
    picture.is_template = is_template;
    Ok(picture)
}

/// An authoritative manifest must name at least one safe variant, including extensionless files.
fn variant(bytes: &[u8], target: Target) -> Result<(String, bool), PictureError> {
    let input = std::str::from_utf8(bytes).map_err(|_| PictureError::Invalid)?;
    let document = roxmltree::Document::parse(input).map_err(|_| PictureError::Invalid)?;
    if !document.root_element().has_tag_name("Picture") {
        return Err(PictureError::Invalid);
    }
    let mut preferred = None;
    for node in document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("PictureVariant"))
    {
        let name = node
            .attribute("name")
            .filter(|name| safe_name(name))
            .ok_or(PictureError::Invalid)?;
        let current = node.attribute("interfaceVariant").is_none_or(str::is_empty);
        // The baseline form profile is Taxi at 96 DPI. The metadata inspector uses full-size art.
        let low_density = target == Target::Form && node.attribute("screenDensity") == Some("ldpi");
        let width = dimension(node, "glyphWidth");
        let height = dimension(node, "glyphHeight");
        let rank = (
            current,
            low_density,
            is_svg(name),
            u64::from(width) * u64::from(height),
            name,
        );
        if preferred.is_none_or(|(previous, _)| rank > previous) {
            let is_template = matches!(node.attribute("isTemplate"), Some("true" | "1"));
            preferred = Some((rank, is_template));
        }
    }
    preferred
        .map(|((_, _, _, _, name), is_template)| (name.to_owned(), is_template))
        .ok_or(PictureError::Unsupported)
}

/// Unknown dimensions rank as zero, retaining valid exports with omitted size attributes.
fn dimension(node: roxmltree::Node<'_, '_>, name: &str) -> u32 {
    node.attribute(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// Without a manifest, prefer SVG or the largest numeric filename; ties are deterministic.
fn fallback_name(archive: &zip::ZipArchive<Cursor<&[u8]>>) -> Result<Option<String>, PictureError> {
    let mut preferred = None;
    for name in archive.file_names() {
        let name = name.map_err(|_| PictureError::Invalid)?;
        if !safe_name(&name) || !supported_name(&name) {
            continue;
        }
        let density = Path::new(name.as_ref())
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u32>().ok())
            .unwrap_or(0);
        let rank = (is_svg(&name), density, name);
        if preferred.as_ref().is_none_or(|previous| rank > *previous) {
            preferred = Some(rank);
        }
    }
    Ok(preferred.map(|(_, _, name)| name.into_owned()))
}

/// Share case-insensitive vector preference between declared and discovered variants.
fn is_svg(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

/// Restrict fallback discovery to formats that the webview can display as images.
fn supported_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        ["png", "jpg", "jpeg", "gif", "bmp", "ico", "svg", "webp"]
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_form_prefers_the_low_density_template_in_the_current_interface() {
        let manifest = br#"<Picture>
          <PictureVariant name="classic.svg" interfaceVariant="Ordinary" screenDensity="ldpi"/>
          <PictureVariant name="l.png" screenDensity="ldpi" isTemplate="true" glyphWidth="16" glyphHeight="16"/>
          <PictureVariant name="m.png" screenDensity="mdpi" glyphWidth="24" glyphHeight="24"/>
          <PictureVariant name="x.png" screenDensity="xdpi" glyphWidth="48" glyphHeight="48"/>
        </Picture>"#;
        assert_eq!(variant(manifest, Target::Form), Ok(("l.png".into(), true)));
        assert_eq!(
            variant(manifest, Target::Metadata),
            Ok(("x.png".into(), false))
        );
    }
}
