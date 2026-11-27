//! Read-only, multi-tip history with snapshot-checked pagination.
use super::repository::{Head, ReferenceTarget, Repository};
use eska_presentation::encoding::json_git_text;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

pub fn history(root: &Path, limit: usize, cursor: Option<&str>) -> Result<Value, Value> {
    if !(1..=200).contains(&limit) {
        return Err(json!("input"));
    }
    let repository = Repository::discover(root).map_err(|_| json!("repository"))?;
    let head = repository.head().map_err(|_| json!("history"))?;
    let mut decorations: BTreeMap<gix::ObjectId, Vec<Value>> = BTreeMap::new();
    let branch = match &head {
        Head::Attached { reference, .. } | Head::Unborn { reference } => Some(json_git_text(
            reference.strip_prefix(b"refs/heads/").unwrap_or(reference),
        )),
        Head::Detached { .. } => None,
    };
    let head_data = json!({"id":head.id().map(|id| id.to_string()),
        "branch":branch.as_ref().map(|(name, _)| name),
        "branch_encoding":branch.as_ref().map(|(_, encoding)| encoding),
        "detached":matches!(head, Head::Detached { .. }),
        "unborn":matches!(head, Head::Unborn { .. })});
    if let Some(id) = head.id() {
        decorations
            .entry(id)
            .or_default()
            .push(json!({"name":"HEAD","name_encoding":"utf-8","kind":"head"}));
    }
    for reference in repository.references().map_err(|_| json!("references"))? {
        let Some((prefix, kind)) = [
            (b"refs/heads/".as_slice(), "local"),
            (b"refs/remotes/", "remote"),
            (b"refs/tags/", "tag"),
        ]
        .into_iter()
        .find(|(prefix, _)| reference.name.starts_with(prefix)) else {
            continue;
        };
        let ReferenceTarget::Object(target) = reference.target else {
            continue;
        };
        let object = repository
            .inner
            .find_object(target)
            .map_err(|_| json!("history"))?;
        // Tags may legitimately refer to trees or blobs, which have no commit ancestry.
        let commit = object.peel_tags_to_end().map_err(|_| json!("history"))?;
        if commit.kind != gix::object::Kind::Commit {
            continue;
        }
        let (name, name_encoding) = json_git_text(&reference.name[prefix.len()..]);
        decorations
            .entry(commit.id)
            .or_default()
            .push(json!({"name":name,"name_encoding":name_encoding,"kind":kind}));
    }
    let shallow = repository
        .inner
        .shallow_commits()
        .map_err(|_| json!("history"))?;
    let shallow_ids = shallow
        .as_ref()
        .map(|ids| ids.iter().map(ToString::to_string).collect::<Vec<_>>());
    let snapshot = json!({"head":head_data,"shallow":shallow_ids,"refs":decorations.iter().map(|(id, refs)| json!([id.to_string(),refs])).collect::<Vec<_>>()});
    let fingerprint = snapshot_fingerprint(&snapshot);
    let offset = parse_cursor(cursor, &fingerprint)?;
    let walk = repository
        .inner
        .rev_walk(decorations.keys().copied())
        .sorting(gix::revision::walk::Sorting::TopoOrder)
        .all()
        .map_err(|_| json!("history"))?;
    let mut entries = Vec::new();
    let mut more = false;
    for (index, item) in walk.enumerate() {
        let info = item.map_err(|_| json!("history"))?;
        if index < offset {
            continue;
        }
        if entries.len() == limit {
            more = true;
            break;
        }
        let commit = repository
            .inner
            .find_commit(info.id)
            .map_err(|_| json!("history"))?;
        let decoded = commit.decode().map_err(|_| json!("history"))?;
        let author = decoded.author().map_err(|_| json!("history"))?;
        let time = author.time().map_err(|_| json!("history"))?;
        let (name, name_encoding) = json_git_text(author.name);
        let (email, email_encoding) = json_git_text(author.email);
        let (subject, subject_encoding) = json_git_text(decoded.message().summary().as_ref());
        entries.push(json!({"id":info.id.to_string(),
            "parents":info.parent_ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "author":{"name":name,"name_encoding":name_encoding,"email":email,"email_encoding":email_encoding},
            "authored_at":{"seconds":time.seconds,"offset":time.offset},
            "subject":subject,"subject_encoding":subject_encoding,
            "refs":decorations.get(&info.id).cloned().unwrap_or_default()}));
    }
    let next_offset = offset
        .checked_add(entries.len())
        .ok_or_else(|| json!("input"))?;
    Ok(json!({"commits":entries,"head":head_data,
        "next_cursor":if more {Some(format!("{fingerprint}:{next_offset}"))} else {None}}))
}

fn snapshot_fingerprint(snapshot: &Value) -> String {
    let mut fingerprint = String::with_capacity(64);
    for byte in Sha256::digest(snapshot.to_string().as_bytes()) {
        use std::fmt::Write as _;
        let _ = write!(fingerprint, "{byte:02x}");
    }
    fingerprint
}

fn parse_cursor(cursor: Option<&str>, fingerprint: &str) -> Result<usize, Value> {
    let Some(cursor) = cursor else { return Ok(0) };
    if cursor.len() > 85 {
        return Err(json!("input"));
    }
    let (hash, offset) = cursor.split_once(':').ok_or_else(|| json!("input"))?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(json!("input"));
    }
    let offset = offset.parse::<usize>().ok().ok_or_else(|| json!("input"))?;
    if hash != fingerprint {
        return Err(json!("stale_cursor"));
    }
    Ok(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursors_are_bounded_and_snapshot_checked() {
        let hash = "a".repeat(64);
        assert_eq!(parse_cursor(None, &hash), Ok(0));
        assert_eq!(parse_cursor(Some(&format!("{hash}:200")), &hash), Ok(200));
        assert_eq!(
            parse_cursor(Some(&format!("{hash}:100001")), &hash),
            Ok(100_001)
        );
        assert_eq!(
            parse_cursor(Some(&format!("{}:1", "b".repeat(64))), &hash),
            Err(json!("stale_cursor"))
        );
        assert_eq!(parse_cursor(Some("HEAD:1"), &hash), Err(json!("input")));
    }
}
