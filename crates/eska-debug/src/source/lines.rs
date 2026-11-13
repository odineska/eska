//! Translate editor coordinates without rewriting the module compiled by 1C.
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

#[derive(Default)]
pub(super) struct Lines {
    shifts: Vec<Shift>,
}

struct Shift {
    platform: u32,
    editor: u32,
}

impl Lines {
    pub(super) fn read(path: &Path) -> io::Result<Self> {
        const LIMIT: u64 = 64 * 1024 * 1024;
        let mut source = Vec::new();
        File::open(path)?.take(LIMIT + 1).read_to_end(&mut source)?;
        if source.len() as u64 > LIMIT {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Self::from_bytes(&source))
    }

    fn from_bytes(source: &[u8]) -> Self {
        let mut shifts = Vec::new();
        let (mut platform, mut editor) = (1, 1);
        let mut offset = 0;
        while offset < source.len() {
            match &source[offset..] {
                [b'\r', b'\r', b'\n', ..] => {
                    // 1C counts CR CR LF once; VS Code exposes two line breaks.
                    // Earlier CRs in a longer run remain independent platform lines.
                    platform += 1;
                    editor += 2;
                    offset += 3;
                    shifts.push(Shift { platform, editor });
                }
                [b'\r', b'\n', ..] => {
                    platform += 1;
                    editor += 1;
                    offset += 2;
                }
                [b'\r' | b'\n', ..] => {
                    platform += 1;
                    editor += 1;
                    offset += 1;
                }
                _ => offset += 1,
            }
        }
        Self { shifts }
    }

    pub(super) fn editor(&self, line: u32) -> u32 {
        let index = self.shifts.partition_point(|shift| shift.platform <= line);
        let extra = index.checked_sub(1).map_or(0, |index| {
            let shift = &self.shifts[index];
            shift.editor - shift.platform
        });
        line.saturating_add(extra)
    }

    pub(super) fn platform(&self, line: u32) -> u32 {
        let index = self.shifts.partition_point(|shift| shift.editor <= line);
        let extra = index.checked_sub(1).map_or(0, |index| {
            let shift = &self.shifts[index];
            shift.editor - shift.platform
        });
        let line = line - extra;
        // An editor-only blank line resolves forward to the next platform line.
        self.shifts
            .get(index)
            .map_or(line, |next| line.min(next.platform))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_and_mixed_endings_preserve_coordinates() {
        for source in [
            b"a\nb\nc\n".as_slice(),
            b"a\r\nb\r\nc\r\n",
            b"a\rb\rc\r",
            b"a\r\nb\rc\n",
            b"a\r\rb\r\rc\r",
        ] {
            let lines = Lines::from_bytes(source);
            assert!(lines.shifts.is_empty());
            for line in 1..=4 {
                assert_eq!(lines.editor(line), line);
                assert_eq!(lines.platform(line), line);
            }
        }
    }

    #[test]
    fn extra_carriage_returns_map_both_directions_and_snap_blank_lines() {
        let lines = Lines::from_bytes(b"a\r\r\nb\nc\r\r\r\nd\r\ne");
        for (platform, editor) in [(1, 1), (2, 3), (3, 4), (4, 5), (5, 7), (6, 8)] {
            assert_eq!(lines.editor(platform), editor);
            assert_eq!(lines.platform(editor), platform);
        }
        assert_eq!(lines.platform(2), 2);
        assert_eq!(lines.platform(5), 4);
        assert_eq!(lines.platform(6), 5);
    }
}
