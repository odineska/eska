//! Snapshot-checked line staging without changing worktree bytes.
use super::{repository::Repository, scm_bridge};
use gix::{
    config::{AsKey, tree::Core},
    diff::blob::{Algorithm, Diff, InternedInput, sources::byte_lines},
    index::entry::{Flags, Mode, Stage, Stat},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::SystemTime,
};

const CONTENT_LIMIT: u64 = 8 * 1024 * 1024;
const INPUT_LIMIT: u64 = 65_536;
type Reply = Result<Value, Value>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    index_sha256: String,
    worktree_sha256: String,
    side: Side,
    ranges: Vec<Range>,
}
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Side {
    Index,
    Worktree,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Range {
    start: usize,
    end: usize,
}

pub(super) fn input(start: &Path, path: &Path) -> Reply {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| json!("input"))?;
    if bytes.len() as u64 > INPUT_LIMIT {
        return Err(json!("input_limit"));
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| json!("input"))?;
    if request.schema_version != 1
        || !hash_valid(&request.index_sha256)
        || !hash_valid(&request.worktree_sha256)
    {
        return Err(json!("input"));
    }
    stage(start, path, &request)
}
fn hash_valid(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn hash(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(result, "{byte:02x}");
    }
    result
}
fn raw_index(repo: &Repository) -> Result<Vec<u8>, Value> {
    match fs::read(repo.index_path()) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(json!("index")),
    }
}
fn worktree(repo: &Repository, path: &Path) -> Result<Option<Vec<u8>>, Value> {
    let full = repo.work_dir().join(path);
    match fs::symlink_metadata(&full) {
        Ok(meta) if meta.is_file() && !meta.is_symlink() => {
            if meta.len() > CONTENT_LIMIT {
                return Err(json!("oversize"));
            }
            let mut bytes = Vec::new();
            fs::File::open(full)
                .map_err(|_| json!("path"))?
                .take(CONTENT_LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| json!("path"))?;
            if bytes.len() as u64 > CONTENT_LIMIT {
                return Err(json!("oversize"));
            }
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        _ => Err(json!("unsupported_text")),
    }
}
fn stage(start: &Path, path: &Path, request: &Request) -> Reply {
    let repo = Repository::discover(start).map_err(|_| json!("repository"))?;
    let mut lock = repo.lock_index().map_err(|_| json!("index_locked"))?;
    scm_bridge::validate(&repo, path)?;
    repo.check_index_size().map_err(|_| json!("index"))?;
    let raw = raw_index(&repo)?;
    let current = worktree(&repo, path)?;
    if hash(current.as_deref().unwrap_or_default()) != request.worktree_sha256 {
        return Err(json!("stale_status"));
    }
    let mut index = decode_index(&repo, &raw)?;
    if index.is_sparse() || index.link().is_some() {
        return Err(json!("index"));
    }
    let key = gix::path::into_bstr(path).map_err(|_| json!("path"))?;
    let key = gix::path::to_unix_separators_on_windows(key);
    validate_index_path(&index, key.as_ref())?;
    // The index writer cannot reproduce the resolve-undo extension yet.
    if index.resolve_undo().is_some() {
        return Err(json!("index"));
    }
    let existing = index.entry_by_path(key.as_ref());
    if existing.is_some_and(|entry| {
        !matches!(entry.mode, Mode::FILE | Mode::FILE_EXECUTABLE)
            || entry
                .flags
                .intersects(Flags::SKIP_WORKTREE | Flags::ASSUME_VALID)
    }) {
        return Err(json!("unsupported_text"));
    }
    let configured = staging_configuration(&repo.inner)?;
    if existing.is_none() {
        let mut excludes = configured
            .excludes(
                &index,
                None,
                gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .map_err(|_| json!("index"))?;
        if excludes
            .at_path(path, None)
            .map_err(|_| json!("path"))?
            .is_excluded()
        {
            return Err(json!("ignored"));
        }
    }
    let mode = existing.map_or_else(
        || new_mode(&configured, &repo.work_dir().join(path)),
        |entry| entry.mode,
    );
    let old = match existing {
        Some(entry) if !entry.flags.contains(Flags::INTENT_TO_ADD) => {
            let header = repo
                .inner
                .find_header(entry.id)
                .map_err(|_| json!("index"))?;
            if header.size() > CONTENT_LIMIT {
                return Err(json!("oversize"));
            }
            repo.inner
                .find_blob(entry.id)
                .map_err(|_| json!("index"))?
                .data
                .clone()
        }
        _ => Vec::new(),
    };
    if hash(&old) != request.index_sha256 {
        return Err(json!("stale_status"));
    }
    text(&old)?;
    let raw_worktree = current.as_deref().unwrap_or_default();
    text(raw_worktree)?;
    let normalized = normalize(&configured, &index, path, raw_worktree)?;
    let selected = select(&old, &normalized, request)?;
    if selected == old {
        return Err(json!("empty_selection"));
    }
    // Recheck after diff/filter work while still excluding conventional index writers.
    scm_bridge::validate(&repo, path)?;
    if raw_index(&repo)? != raw || worktree(&repo, path)? != current {
        return Err(json!("stale_status"));
    }
    let id = repo
        .inner
        .write_blob(&selected)
        .map_err(|_| json!("mutation"))?
        .detach();
    index.remove_entries(|_, entry_path, _| entry_path == key.as_ref());
    if current.is_some() || !selected.is_empty() {
        index.dangerously_push_entry(Stat::default(), id, Flags::empty(), mode, key.as_ref());
        index.sort_entries();
    }
    gix::index::File::from_state(index, repo.index_path())
        .write_to(&mut lock, gix::index::write::Options::default())
        .map_err(|_| json!("mutation"))?;
    lock.flush().map_err(|_| json!("mutation"))?;
    lock.commit().map_err(|_| json!("mutation"))?;
    Ok(json!({}))
}
fn validate_index_path(index: &gix::index::State, path: &[u8]) -> Result<(), Value> {
    let descendant =
        |a: &[u8], b: &[u8]| a.strip_prefix(b).is_some_and(|rest| rest.starts_with(b"/"));
    for entry in index.entries() {
        let other: &[u8] = entry.path(index).as_ref();
        if other == path && entry.stage() != Stage::Unconflicted {
            return Err(json!("conflict"));
        }
        // Adding a file/directory replacement would also remove unselected index entries.
        if descendant(path, other) || descendant(other, path) {
            return Err(json!("path"));
        }
    }
    Ok(())
}
fn decode_index(repo: &Repository, raw: &[u8]) -> Result<gix::index::State, Value> {
    if raw.is_empty() {
        return Ok(gix::index::State::new(repo.inner.object_hash()));
    }
    gix::index::State::from_bytes(
        raw,
        SystemTime::now().into(),
        repo.inner.object_hash(),
        gix::index::decode::Options::default(),
    )
    .map(|(state, _)| state)
    .map_err(|_| json!("index"))
}
fn new_mode(repo: &gix::Repository, path: &Path) -> Mode {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if repo.config_snapshot().boolean(Core::FILE_MODE) != Some(false)
            && fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o100 != 0)
        {
            return Mode::FILE_EXECUTABLE;
        }
    }
    let _ = (repo, path);
    Mode::FILE
}
fn text(bytes: &[u8]) -> Result<(), Value> {
    if byte_lines(bytes).take(200_001).count() > 200_000 {
        return Err(json!("oversize"));
    }
    if bytes.contains(&0)
        || std::str::from_utf8(bytes).is_err()
        || bytes
            .iter()
            .enumerate()
            .any(|(position, byte)| *byte == b'\r' && bytes.get(position + 1) != Some(&b'\n'))
    {
        return Err(json!("unsupported_text"));
    }
    Ok(())
}
fn staging_configuration(repo: &gix::Repository) -> Result<gix::Repository, Value> {
    let effective =
        super::status::effective_git_config(repo).map_err(|_| json!("unsupported_text"))?;
    let mut configured = repo.clone();
    let mut config = configured.config_snapshot_mut();
    let keys: [&'static dyn gix::config::tree::Key; 6] = [
        &Core::AUTO_CRLF,
        &Core::EOL,
        &Core::SAFE_CRLF,
        &Core::FILE_MODE,
        &Core::ATTRIBUTES_FILE,
        &Core::EXCLUDES_FILE,
    ];
    for key in keys {
        if let Some(value) = effective.string_implicit(key.as_key()) {
            config
                .set_value(key, value.unwrap_or_else(|| "true".into()))
                .map_err(|_| json!("unsupported_text"))?;
        }
    }
    config.commit().map_err(|_| json!("unsupported_text"))?;
    Ok(configured)
}
fn normalize(
    repo: &gix::Repository,
    index: &gix::index::State,
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<u8>, Value> {
    let mut attributes = repo
        .attributes_only(
            index,
            gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
        )
        .map_err(|_| json!("unsupported_text"))?;
    let mut outcome = gix::attrs::search::Outcome::default();
    attributes
        .at_path(path, None)
        .map_err(|_| json!("unsupported_text"))?
        .matching_attributes(&mut outcome);
    if outcome
        .iter()
        .any(|attribute| match attribute.assignment.name.as_str() {
            "filter" | "working-tree-encoding" | "ident" => !matches!(
                attribute.assignment.state,
                gix::attrs::StateRef::Unspecified | gix::attrs::StateRef::Unset
            ),
            "diff" => !matches!(
                attribute.assignment.state,
                gix::attrs::StateRef::Unspecified | gix::attrs::StateRef::Set
            ),
            _ => false,
        })
    {
        return Err(json!("unsupported_text"));
    }
    let mut pipeline = gix::filter::Pipeline::new(repo, attributes.detach())
        .map_err(|_| json!("unsupported_text"))?;
    let mut normalized = Vec::new();
    pipeline
        .convert_to_git(bytes, path, index)
        .map_err(|_| json!("unsupported_text"))?
        .take(CONTENT_LIMIT + 1)
        .read_to_end(&mut normalized)
        .map_err(|_| json!("unsupported_text"))?;
    // Only CRLF removal preserves the raw editor line offsets used by this protocol.
    let crlf_removed = bytes.iter().enumerate().filter_map(|(position, byte)| {
        (!(*byte == b'\r' && bytes.get(position + 1) == Some(&b'\n'))).then_some(*byte)
    });
    if normalized != bytes && !crlf_removed.eq(normalized.iter().copied()) {
        return Err(json!("unsupported_text"));
    }
    Ok(normalized)
}
fn select(old: &[u8], new: &[u8], request: &Request) -> Result<Vec<u8>, Value> {
    let before = byte_lines(old).collect::<Vec<_>>();
    let after = byte_lines(new).collect::<Vec<_>>();
    let length = match request.side {
        Side::Index => before.len(),
        Side::Worktree => after.len(),
    };
    if request.ranges.is_empty() {
        return Err(json!("empty_selection"));
    }
    if request.ranges.len() > 2048
        || request
            .ranges
            .iter()
            .any(|range| range.start >= range.end || range.end > length)
    {
        return Err(json!("input"));
    }
    let mut selection = vec![false; length];
    for range in &request.ranges {
        selection[range.start..range.end].fill(true);
    }
    let selected = |line: usize| selection[line];
    let input = InternedInput::new(byte_lines(old), byte_lines(new));
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let mut result = Vec::with_capacity(new.len());
    let mut cursor = 0;
    let mut changed = false;
    for hunk in diff.hunks() {
        let a = hunk.before.start as usize..hunk.before.end as usize;
        let b = hunk.after.start as usize..hunk.after.end as usize;
        for line in &before[cursor..a.start] {
            result.extend_from_slice(line);
        }
        let side = match request.side {
            Side::Index => a.clone(),
            Side::Worktree => b.clone(),
        };
        let count = side.clone().filter(|line| selected(*line)).count();
        if count == 0 {
            for line in &before[a.clone()] {
                result.extend_from_slice(line);
            }
        } else if a.len() == b.len() {
            for offset in 0..a.len() {
                let line = if selected(side.start + offset) {
                    changed = true;
                    after[b.start + offset]
                } else {
                    before[a.start + offset]
                };
                result.extend_from_slice(line);
            }
        } else if a.is_empty() {
            for position in b.filter(|line| selected(*line)) {
                result.extend_from_slice(after[position]);
                changed = true;
            }
        } else if b.is_empty() {
            for position in a.clone() {
                if selected(position) {
                    changed = true;
                } else {
                    result.extend_from_slice(before[position]);
                }
            }
        } else if count == side.len() {
            for line in &after[b] {
                result.extend_from_slice(line);
            }
            changed = true;
        } else {
            return Err(json!("ambiguous_lines"));
        }
        cursor = a.end;
    }
    for line in &before[cursor..] {
        result.extend_from_slice(line);
    }
    if !changed {
        return Err(json!("empty_selection"));
    }
    Ok(result)
}
