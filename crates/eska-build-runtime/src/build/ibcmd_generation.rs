//! Read the generation produced by a successful file-mode ibcmd apply.

const CONFIGURATION_RECORDS: &[&str] = &[
    "[INFO] Configuration generation is created:",
    "[INFO] Created configuration generation:",
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

/// Retain only authoritative generation evidence while draining arbitrarily noisy stdout.
/// Matching malformed or repeated records invalidate the evidence, including after overflow.
#[derive(Default)]
pub struct Collector {
    line: Vec<u8>,
    discarded: bool,
    record: Option<String>,
    matched: bool,
    invalid: bool,
    utf8: Vec<u8>,
    utf8_width: usize,
}

impl Collector {
    pub fn push(&mut self, bytes: &[u8], extension: bool) {
        for &byte in bytes {
            self.validate_utf8(byte);
            if byte == b'\n' {
                self.flush(extension);
            } else if !self.discarded {
                if self.line.len() == 256 {
                    // Keep the prefix so an oversized malformed generation record cannot disappear.
                    self.discarded = true;
                } else {
                    self.line.push(byte);
                }
            }
        }
    }

    fn validate_utf8(&mut self, byte: u8) {
        if self.invalid {
            return;
        }
        if self.utf8.is_empty() {
            if byte.is_ascii() {
                return;
            }
            self.utf8_width = match byte {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => {
                    self.invalid = true;
                    return;
                }
            };
        }
        self.utf8.push(byte);
        if self.utf8.len() == self.utf8_width {
            self.invalid |= std::str::from_utf8(&self.utf8).is_err();
            self.utf8.clear();
        }
    }

    fn flush(&mut self, extension: bool) {
        let prefixes = if extension {
            EXTENSION_RECORDS
        } else {
            CONFIGURATION_RECORDS
        };
        for prefix in prefixes {
            if let Some(value) = self.line.strip_prefix(prefix.as_bytes()) {
                if self.matched || self.discarded {
                    self.invalid = true;
                }
                self.matched = true;
                self.record = std::str::from_utf8(value).ok().and_then(parse_id);
                self.invalid |= self.record.is_none();
                break;
            }
        }
        self.line.clear();
        self.discarded = false;
    }

    pub fn finish(mut self, extension: bool) -> Option<String> {
        self.flush(extension);
        if self.invalid || !self.utf8.is_empty() {
            None
        } else {
            self.record
        }
    }
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

#[cfg(test)]
mod streaming_tests {
    use super::*;

    #[test]
    fn generation_evidence_survives_noise_but_not_duplicates_or_invalid_records() {
        let id = "a".repeat(40);
        for extension in [false, true] {
            let prefixes = if extension {
                EXTENSION_RECORDS
            } else {
                CONFIGURATION_RECORDS
            };
            for prefix in prefixes {
                let record = format!("{prefix} {id}\n");
                let mut collector = Collector::default();
                collector.push(&vec![b'x'; 1024 * 1024 + 1], extension);
                collector.push(b"\n", extension);
                for byte in record.as_bytes() {
                    collector.push(&[*byte], extension);
                }
                collector.push(&vec![b'y'; 1024 * 1024 + 1], extension);
                assert_eq!(collector.finish(extension).as_deref(), Some(id.as_str()));
                for suffix in [
                    record.clone().into_bytes(),
                    format!("{prefix} invalid\n").into_bytes(),
                    format!("{prefix} {}\n", "x".repeat(1024)).into_bytes(),
                    vec![0xff],
                ] {
                    let mut collector = Collector::default();
                    collector.push(record.as_bytes(), extension);
                    collector.push(&vec![b'x'; 1024 * 1024 + 1], extension);
                    collector.push(b"\n", extension);
                    collector.push(&suffix, extension);
                    assert_eq!(collector.finish(extension), None);
                }
            }
        }
    }
}
