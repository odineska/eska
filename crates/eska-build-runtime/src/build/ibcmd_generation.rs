//! Read the generation produced by a successful file-mode ibcmd apply.

const CONFIGURATION_RECORDS: &[&str] = &[
    "[INFO] Configuration generation is created:",
    "[INFO] Создано поколение конфигурации:",
];
const EXTENSION_RECORDS: &[&str] = &["[INFO] Создано поколение расширения конфигурации:"];

pub fn parse_id(text: &str) -> Option<String> {
    let id = text.trim();
    (id.len() == 40 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| id.to_ascii_lowercase())
}

pub fn from_apply_output(output: &[u8], extension: bool) -> Option<String> {
    let prefixes = if extension {
        EXTENSION_RECORDS
    } else {
        CONFIGURATION_RECORDS
    };
    let mut records = std::str::from_utf8(output)
        .ok()?
        .lines()
        .filter_map(|line| prefixes.iter().find_map(|prefix| line.strip_prefix(prefix)));
    // Use only an unambiguous record for the requested component. Other
    // locales, unchanged configurations and unexpected output use generation-id.
    let id = parse_id(records.next()?)?;
    records.next().is_none().then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "99a38e8683135742b7041e3deb18705800000000";

    #[test]
    fn accepts_exact_records_for_the_requested_component() {
        for prefix in CONFIGURATION_RECORDS {
            let output = format!("[INFO] Updating...\r\n{prefix} {}\r\n", ID.to_uppercase());
            assert_eq!(
                from_apply_output(output.as_bytes(), false).as_deref(),
                Some(ID)
            );
            assert_eq!(from_apply_output(output.as_bytes(), true), None);
        }
        let output = format!("{} {ID}\n", EXTENSION_RECORDS[0]);
        assert_eq!(
            from_apply_output(output.as_bytes(), true).as_deref(),
            Some(ID)
        );
        assert_eq!(from_apply_output(output.as_bytes(), false), None);
    }

    #[test]
    fn rejects_missing_malformed_and_ambiguous_records() {
        let prefix = CONFIGURATION_RECORDS[0];
        for output in [
            String::new(),
            format!("[WARNING] {ID}"),
            format!("{prefix} {ID} extra"),
            format!("{prefix} {ID}\n{prefix} {ID}"),
            format!("{prefix} {ID}\n{prefix} unknown"),
        ] {
            assert_eq!(from_apply_output(output.as_bytes(), false), None);
        }
        assert_eq!(from_apply_output(b"\xff", false), None);
    }
}
