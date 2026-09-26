//! Windows: the file type filter of the open and save dialogs (the portable part
//! of `dialogs.rs`).

/// A file type filter for `IFileDialog::SetFileTypes`: its display name and its
/// `;`-separated patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Filter {
    pub name: String,
    pub spec: String,
}

/// One filter matching every extension in `extensions` (lowercase, without the
/// dot; duplicates ignored), or `None` to allow every file.
pub(super) fn image_filter(extensions: &[String]) -> Option<Filter> {
    let mut patterns: Vec<String> = Vec::new();
    for extension in extensions {
        let pattern = format!("*.{extension}");
        if !patterns.contains(&pattern) {
            patterns.push(pattern);
        }
    }
    (!patterns.is_empty()).then(|| Filter {
        name: format!("Images ({})", patterns.join(", ")),
        spec: patterns.join(";"),
    })
}

/// The extension the save dialog appends when the user types a name without an
/// allowed one: the suggested name's own extension if it is allowed, else the
/// first allowed extension. `None` if any extension is allowed.
pub(super) fn default_extension<'a>(file_name: &str, extensions: &'a [String]) -> Option<&'a str> {
    let suggested = file_name.rsplit_once('.').map(|(_, extension)| extension);
    extensions
        .iter()
        .find(|allowed| suggested.is_some_and(|suggested| suggested.eq_ignore_ascii_case(allowed)))
        .or_else(|| extensions.first())
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn one_filter_matches_every_extension_once() {
        let filter = image_filter(&strings(&["png", "jpg", "jpeg", "png"])).unwrap();
        assert_eq!(filter.spec, "*.png;*.jpg;*.jpeg");
        assert_eq!(filter.name, "Images (*.png, *.jpg, *.jpeg)");
    }

    #[test]
    fn no_extensions_means_no_filter() {
        assert_eq!(image_filter(&[]), None);
    }

    #[test]
    fn default_extension_keeps_the_suggested_one_if_allowed() {
        let allowed = strings(&["png", "jpg", "webp"]);
        assert_eq!(default_extension("Screenshot.webp", &allowed), Some("webp"));
        assert_eq!(default_extension("Screenshot.JPG", &allowed), Some("jpg"));
        assert_eq!(default_extension("Screenshot.tiff", &allowed), Some("png"));
        assert_eq!(default_extension("Screenshot", &allowed), Some("png"));
        assert_eq!(default_extension("Screenshot.png", &[]), None);
    }
}
