//! Shelf index integrity and session lock lifetime.

use super::*;

#[test]
fn captured_index_cannot_protect_an_incomplete_tree() {
    use gix::index::entry::{Flags, Mode, Stat};

    let root = crate::test_support::TestDir::new();
    gix::init(&root.0).unwrap();
    let repository = Repository::discover(&root.0).unwrap();
    let hash = repository.inner.object_hash();
    let blob = repository
        .inner
        .write_blob(b"staged data")
        .unwrap()
        .detach();
    let missing = gix::objs::compute_hash(hash, gix::objs::Kind::Blob, b"missing data").unwrap();
    let snapshot = root.0.join("snapshot");
    let save = |entries: &[(&str, ObjectId, Flags, Mode)]| {
        let mut state = gix::index::State::new(hash);
        for &(path, id, flags, mode) in entries {
            state.dangerously_push_entry(Stat::default(), id, flags, mode, path.into());
        }
        state.sort_entries();
        let file = gix::index::File::from_state(state, &snapshot);
        file.write_to(
            File::create(&snapshot).unwrap(),
            gix::index::write::Options::default(),
        )
        .unwrap();
    };

    save(&[]);
    assert_eq!(
        index_tree(&repository, &snapshot).unwrap(),
        ObjectId::empty_tree(hash)
    );
    save(&[("missing", missing, Flags::empty(), Mode::FILE)]);
    assert!(matches!(
        index_tree(&repository, &snapshot),
        Err(Error::InvalidShelf)
    ));
    for entries in [
        vec![("null", ObjectId::null(hash), Flags::empty(), Mode::FILE)],
        vec![(
            "intent",
            blob,
            Flags::EXTENDED | Flags::INTENT_TO_ADD,
            Mode::FILE,
        )],
        vec![("gitlink", blob, Flags::empty(), Mode::COMMIT)],
        vec![(
            "sparse",
            blob,
            Flags::EXTENDED | Flags::SKIP_WORKTREE,
            Mode::DIR,
        )],
        vec![(
            "conflict",
            blob,
            gix::index::entry::Stage::Ours.into(),
            Mode::FILE,
        )],
        vec![
            ("node", blob, Flags::empty(), Mode::FILE),
            ("node/child", blob, Flags::empty(), Mode::FILE),
        ],
    ] {
        save(&entries);
        let result = index_tree(&repository, &snapshot);
        assert!(
            matches!(result, Err(Error::UnsupportedIndex)),
            "{entries:?}: {result:?}"
        );
    }
}

/// The session owns the mutation lock even if another descriptor still refers to its open file.
#[test]
fn session_drop_releases_lock_before_a_duplicated_descriptor_closes() {
    let root = crate::test_support::TestDir::new();
    gix::init(&root.0).unwrap();
    let repository = Repository::discover(&root.0).unwrap();
    let session = Session::acquire(&repository).unwrap();
    let duplicated = session.lock.try_clone().unwrap();
    drop(session);
    let next = Session::acquire(&repository).expect("the completed session released its lock");
    assert!(duplicated.metadata().unwrap().is_file());
    drop(next);
    drop(duplicated);
}
