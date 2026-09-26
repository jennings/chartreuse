//! The FileChooser portal's file filters and the `file://` URIs it answers
//! with.

/// A case-insensitive glob pattern for files ending in `.extension`
/// (`png` → `*.[pP][nN][gG]`): the portal backends match patterns
/// case-sensitively, and cameras and Windows tools write `.PNG` and `.JPG`.
#[must_use]
pub fn extension_glob(extension: &str) -> String {
    let mut glob = String::from("*.");
    for character in extension.chars() {
        let (lower, upper) = (
            character.to_ascii_lowercase(),
            character.to_ascii_uppercase(),
        );
        if lower == upper {
            glob.push(character);
        } else {
            glob.extend(['[', lower, upper, ']']);
        }
    }
    glob
}

/// The bytes of the local path a `file://` URI names, percent-decoded (Unix
/// paths are bytes and need not be UTF-8); `None` for other schemes, remote
/// hosts, and malformed escapes.
#[must_use]
pub fn file_uri_path(uri: &str) -> Option<Vec<u8>> {
    let rest = uri.strip_prefix("file://")?;
    // An authority other than empty or `localhost` names another machine.
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(slash) if &rest[..slash] == "localhost" => &rest[slash..],
        _ => return None,
    };
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = bytes.get(index + 1..index + 3)?;
                let hex = std::str::from_utf8(hex).ok()?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            // A query or fragment has no place in a file URI.
            b'?' | b'#' => return None,
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_globs_match_either_case() {
        assert_eq!(extension_glob("png"), "*.[pP][nN][gG]");
        assert_eq!(extension_glob("mp4"), "*.[mM][pP]4");
    }

    #[test]
    fn file_uris_decode_to_local_paths() {
        let path =
            |uri| file_uri_path(uri).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        assert_eq!(
            path("file:///home/me/My%20Pictures/shot%231.png").as_deref(),
            Some("/home/me/My Pictures/shot#1.png")
        );
        assert_eq!(
            path("file://localhost/tmp/a.png").as_deref(),
            Some("/tmp/a.png")
        );
        // Not UTF-8: Latin-1 "é".
        assert_eq!(
            file_uri_path("file:///tmp/caf%E9.png").as_deref(),
            Some(&b"/tmp/caf\xe9.png"[..])
        );
    }

    #[test]
    fn other_uris_are_not_local_paths() {
        for uri in [
            "https://example.com/a.png",
            "file://server/share/a.png",
            "file:///tmp/%zz.png",
            "file:///tmp/truncated%2",
            "file:///tmp/a.png?query",
            "file:relative.png",
        ] {
            assert_eq!(file_uri_path(uri), None, "{uri}");
        }
    }
}
