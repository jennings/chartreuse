//! `Info.plist` generation for the macOS app bundle.

use std::fmt::Write as _;

/// The macOS version Chartreuse requires (`SCScreenshotManager` needs 14.0).
pub const MINIMUM_SYSTEM_VERSION: &str = "14.0";

/// The values that differ between bundles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoPlist<'a> {
    pub bundle_id: &'a str,
    pub display_name: &'a str,
    /// The file name in `Contents/MacOS/`.
    pub executable: &'a str,
    /// The file name in `Contents/Resources/`, without `.icns`.
    pub icon_file: &'a str,
    pub version: &'a str,
}

enum Value<'a> {
    String(&'a str),
    Bool(bool),
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            c => escaped.push(c),
        }
    }
    escaped
}

impl InfoPlist<'_> {
    fn entries(&self) -> [(&'static str, Value<'_>); 13] {
        [
            ("CFBundleDevelopmentRegion", Value::String("en")),
            ("CFBundleDisplayName", Value::String(self.display_name)),
            ("CFBundleExecutable", Value::String(self.executable)),
            ("CFBundleIconFile", Value::String(self.icon_file)),
            ("CFBundleIdentifier", Value::String(self.bundle_id)),
            ("CFBundleInfoDictionaryVersion", Value::String("6.0")),
            ("CFBundleName", Value::String(self.display_name)),
            ("CFBundlePackageType", Value::String("APPL")),
            ("CFBundleShortVersionString", Value::String(self.version)),
            ("CFBundleVersion", Value::String(self.version)),
            (
                "LSMinimumSystemVersion",
                Value::String(MINIMUM_SYSTEM_VERSION),
            ),
            // No Dock icon: Chartreuse lives in the menu bar.
            ("LSUIElement", Value::Bool(true)),
            ("NSHighResolutionCapable", Value::Bool(true)),
        ]
    }

    /// The XML property list.
    #[must_use]
    pub fn render(&self) -> String {
        let mut xml = String::from(concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n<dict>\n",
        ));
        for (key, value) in self.entries() {
            let _ = writeln!(xml, "\t<key>{key}</key>");
            let _ = match value {
                Value::String(text) => writeln!(xml, "\t<string>{}</string>", escape(text)),
                Value::Bool(true) => writeln!(xml, "\t<true/>"),
                Value::Bool(false) => writeln!(xml, "\t<false/>"),
            };
        }
        xml.push_str("</dict>\n</plist>\n");
        xml
    }
}

#[cfg(test)]
mod tests {
    use plist::{Dictionary, Value};

    use super::*;

    fn parse(info: &InfoPlist<'_>) -> Dictionary {
        Value::from_reader_xml(info.render().as_bytes())
            .expect("valid XML plist")
            .into_dictionary()
            .expect("top-level dictionary")
    }

    fn sample() -> InfoPlist<'static> {
        InfoPlist {
            bundle_id: "io.jennings.chartreuse.dev",
            display_name: "Chartreuse Dev",
            executable: "chartreuse",
            icon_file: "AppIcon",
            version: "0.1.0",
        }
    }

    #[test]
    fn plist_declares_a_menu_bar_app_for_macos_14() {
        let dict = parse(&sample());
        let string = |key: &str| dict.get(key).and_then(Value::as_string).map(str::to_owned);
        assert_eq!(
            dict.get("LSUIElement").and_then(Value::as_boolean),
            Some(true)
        );
        assert_eq!(string("LSMinimumSystemVersion").as_deref(), Some("14.0"));
        assert_eq!(
            string("CFBundleIdentifier").as_deref(),
            Some("io.jennings.chartreuse.dev")
        );
        assert_eq!(string("CFBundleExecutable").as_deref(), Some("chartreuse"));
        assert_eq!(string("CFBundleName").as_deref(), Some("Chartreuse Dev"));
        assert_eq!(string("CFBundlePackageType").as_deref(), Some("APPL"));
        assert_eq!(string("CFBundleVersion").as_deref(), Some("0.1.0"));
        assert_eq!(
            string("CFBundleShortVersionString").as_deref(),
            Some("0.1.0")
        );
        assert_eq!(string("CFBundleIconFile").as_deref(), Some("AppIcon"));
    }

    #[test]
    fn markup_in_values_is_escaped() {
        let info = InfoPlist {
            display_name: "Tom & <Jerry> \"Q's\"",
            ..sample()
        };
        let dict = parse(&info);
        assert_eq!(
            dict.get("CFBundleDisplayName").and_then(Value::as_string),
            Some("Tom & <Jerry> \"Q's\"")
        );
    }
}
