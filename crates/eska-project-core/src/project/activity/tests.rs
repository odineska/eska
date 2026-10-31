use super::*;
use crate::test_support::TestDir;
use std::fs;

fn summary() -> RootSummary {
    RootSummary {
        id: serde_json::from_str("\"report:Демо\"").unwrap(),
        name: "Демо".into(),
    }
}

#[test]
fn missing_activity_is_read_only_and_new_members_are_active() {
    let root = TestDir::new();
    let store = Store::new(&root.0.join("machine"), &root.0).unwrap();
    let snapshot = store.load().unwrap();
    assert!(snapshot.state.enabled(&root.0.join("new")));
    assert!(!snapshot.state.hide_disabled);
    assert!(!root.0.join("machine").exists());
}

#[test]
fn project_activity_and_visibility_are_saved_together_without_source_writes() {
    let root = TestDir::new();
    let store = Store::new(&root.0.join("machine"), &root.0).unwrap();
    let mut first = store.load().unwrap();
    let member = root.0.join("member");
    first.state.disabled.insert(path_key(&member), summary());
    first.state.hide_disabled = true;
    let saved = store.save(&first.revision, &first.state).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(saved.revision, loaded.revision);
    assert_eq!(saved.state, loaded.state);
    assert!(!loaded.state.enabled(&member));
    assert!(loaded.state.enabled(&root.0.join("other")));
    assert!(!member.exists());
    let mut enabled = loaded.state.clone();
    enabled.disabled.remove(&path_key(&member));
    let enabled = store.save(&loaded.revision, &enabled).unwrap();
    assert!(enabled.state.enabled(&member));
    assert!(enabled.state.hide_disabled);
    assert_eq!(
        fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn stale_writers_and_damaged_state_never_overwrite_the_current_file() {
    let root = TestDir::new();
    let store = Store::new(&root.0.join("machine"), &root.0).unwrap();
    let first = store.load().unwrap();
    let mut state = first.state.clone();
    state.hide_disabled = true;
    let saved = store.save(&first.revision, &state).unwrap();
    assert!(matches!(
        store.save(&first.revision, &first.state),
        Err(Error::Conflict)
    ));
    assert_eq!(store.load().unwrap().revision, saved.revision);
    fs::write(store.path(), b"invalid user state").unwrap();
    assert!(matches!(store.load(), Err(Error::Invalid)));
    assert!(matches!(
        store.save(&saved.revision, &state),
        Err(Error::Invalid)
    ));
    assert_eq!(fs::read(store.path()).unwrap(), b"invalid user state");
}

#[test]
fn schemas_keys_and_oversized_records_are_rejected() {
    let root = TestDir::new();
    let store = Store::new(&root.0.join("machine"), &root.0).unwrap();
    let first = store.load().unwrap();
    let mut state = first.state.clone();
    state.schema_version = 2;
    assert!(matches!(
        store.save(&first.revision, &state),
        Err(Error::Invalid)
    ));
    state.schema_version = 1;
    state.disabled.insert("../foreign".into(), summary());
    assert!(matches!(
        store.save(&first.revision, &state),
        Err(Error::Invalid)
    ));
    state.disabled.clear();
    let mut oversized = summary();
    oversized.name = "x".repeat(65_537);
    state.disabled.insert(path_key(&root.0), oversized);
    assert!(matches!(
        store.save(&first.revision, &state),
        Err(Error::Invalid)
    ));
    assert!(!store.path().exists());
}

#[test]
fn workspaces_have_separate_namespaces_and_native_lock_contention_is_explicit() {
    let root = TestDir::new();
    let base = root.0.join("machine");
    let store = Store::new(&base, &root.0.join("one")).unwrap();
    assert_ne!(
        store.path(),
        Store::new(&base, &root.0.join("two")).unwrap().path()
    );
    let first = store.load().unwrap();
    let mut state = first.state.clone();
    state.hide_disabled = true;
    store.save(&first.revision, &state).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(store.path().parent().unwrap().join("activity.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(matches!(
        store.save(&store.load().unwrap().revision, &state),
        Err(Error::Busy)
    ));
    lock.unlock().unwrap();
}

#[cfg(unix)]
#[test]
fn a_state_symlink_is_never_followed_or_replaced() {
    let root = TestDir::new();
    let store = Store::new(&root.0.join("machine"), &root.0).unwrap();
    let first = store.load().unwrap();
    let mut state = first.state.clone();
    state.hide_disabled = true;
    store.save(&first.revision, &state).unwrap();
    fs::remove_file(store.path()).unwrap();
    let foreign = root.0.join("foreign");
    fs::write(&foreign, "keep").unwrap();
    std::os::unix::fs::symlink(&foreign, store.path()).unwrap();
    assert!(matches!(store.load(), Err(Error::UnsafePath)));
    assert!(matches!(
        store.save(&first.revision, &state),
        Err(Error::UnsafePath)
    ));
    assert_eq!(fs::read(&foreign).unwrap(), b"keep");
}
