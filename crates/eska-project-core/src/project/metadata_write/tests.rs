use super::*;
use crate::test_support::TestDir;

fn fixture() -> (TestDir, PathBuf, Vec<FileChange>, Vec<DirectoryChange>) {
    let directory = TestDir::new();
    let source = directory.0.join("src");
    fs::create_dir_all(source.join("Old/Empty")).unwrap();
    fs::write(source.join("Root.xml"), b"\xef\xbb\xbf<Old/>\r\n").unwrap();
    fs::write(source.join("Old/image.bin"), [0, 255, 17, 9]).unwrap();
    let files = vec![
        FileChange {
            path: "Root.xml".into(),
            before: Some(b"\xef\xbb\xbf<Old/>\r\n".to_vec()),
            after: Some(b"\xef\xbb\xbf<New/>\r\n".to_vec()),
        },
        FileChange {
            path: "Old/image.bin".into(),
            before: Some(vec![0, 255, 17, 9]),
            after: None,
        },
        FileChange {
            path: "New/image.bin".into(),
            before: None,
            after: Some(vec![0, 255, 17, 9]),
        },
    ];
    let directories = vec![
        DirectoryChange {
            path: "Old".into(),
            before: true,
            after: false,
        },
        DirectoryChange {
            path: "Old/Empty".into(),
            before: true,
            after: false,
        },
        DirectoryChange {
            path: "New".into(),
            before: false,
            after: true,
        },
        DirectoryChange {
            path: "New/Empty".into(),
            before: false,
            after: true,
        },
    ];
    (directory, source, files, directories)
}

fn stage(
    guard: &Guard,
    source: &Path,
    files: &[FileChange],
    directories: &[DirectoryChange],
) -> (PathBuf, Journal) {
    let staging = guard.journal_root().join(DIRECTORY);
    fs::create_dir(&staging).unwrap();
    let journal = prepare(source, files, directories, &staging).unwrap();
    (staging, journal)
}

#[test]
fn publishes_binary_resources_and_empty_folders_without_rewriting_text_style() {
    let (root, source, files, directories) = fixture();
    let guard = Guard::acquire(&root.0).unwrap();
    guard.publish_files(&source, &files, &directories).unwrap();
    assert_eq!(
        fs::read(source.join("Root.xml")).unwrap(),
        b"\xef\xbb\xbf<New/>\r\n"
    );
    assert_eq!(
        fs::read(source.join("New/image.bin")).unwrap(),
        [0, 255, 17, 9]
    );
    assert!(source.join("New/Empty").is_dir());
    assert!(!source.join("Old").exists());
    assert!(!guard.recovery_status(&source).unwrap().pending);
}

#[test]
fn full_preflight_detects_changed_input_before_any_source_mutation() {
    let (root, source, files, directories) = fixture();
    fs::write(source.join("Old/image.bin"), b"external").unwrap();
    let guard = Guard::acquire(&root.0).unwrap();
    assert!(matches!(
        guard.publish_files(&source, &files, &directories),
        Err(EditError::Conflict)
    ));
    assert_eq!(
        fs::read(source.join("Root.xml")).unwrap(),
        files[0].before.as_ref().unwrap().as_slice()
    );
    assert!(!source.join("New").exists());
}

#[test]
fn interrupted_publication_blocks_other_metadata_writers_and_recovers_exact_bytes() {
    let (root, source, files, directories) = fixture();
    let guard = Guard::acquire(&root.0).unwrap();
    let (staging, journal) = stage(&guard, &source, &files, &directories);
    fs::create_dir(source.join("New")).unwrap();
    write_file(&journal, &staging, 0, &journal.files[0], false).unwrap();
    write_file(&journal, &staging, 1, &journal.files[1], false).unwrap();
    drop(guard);
    let guard = Guard::acquire(&root.0).unwrap();
    assert!(matches!(
        guard.check_clear(&source),
        Err(EditError::RecoveryRequired)
    ));
    let status = guard.recovery_status(&source).unwrap();
    assert!(status.pending && status.conflicts.is_empty());
    guard
        .recover(&source, status.snapshot.as_deref().unwrap())
        .unwrap();
    for file in &files {
        assert_eq!(fs::read(source.join(&file.path)).ok(), file.before);
    }
    assert!(source.join("Old/Empty").is_dir());
    assert!(!source.join("New").exists());
}

#[test]
fn recovery_refuses_to_erase_external_edits_and_does_not_alias_journal_payloads() {
    let (root, source, files, directories) = fixture();
    let guard = Guard::acquire(&root.0).unwrap();
    let (staging, journal) = stage(&guard, &source, &files, &directories);
    commit(&journal, &staging).unwrap();
    fs::write(source.join("New/image.bin"), b"external").unwrap();
    assert_eq!(fs::read(staging.join("2.after")).unwrap(), [0, 255, 17, 9]);
    let status = guard.recovery_status(&source).unwrap();
    assert_eq!(status.conflicts, [PathBuf::from("New/image.bin")]);
    assert!(matches!(
        guard.recover(&source, status.snapshot.as_deref().unwrap()),
        Err(EditError::Conflict)
    ));
    assert_eq!(fs::read(source.join("New/image.bin")).unwrap(), b"external");
}

#[test]
fn corrupt_staged_bytes_never_reach_sources() {
    let (root, source, files, directories) = fixture();
    let guard = Guard::acquire(&root.0).unwrap();
    let (staging, _) = stage(&guard, &source, &files, &directories);
    fs::write(staging.join("0.before"), b"invalid").unwrap();
    assert!(matches!(
        guard.recovery_status(&source),
        Err(EditError::RecoveryRequired)
    ));
    assert_eq!(
        fs::read(source.join("Root.xml")).unwrap(),
        files[0].before.as_ref().unwrap().as_slice()
    );
}

#[test]
fn completed_publication_can_be_cleaned_after_sources_have_been_edited_again() {
    let (root, source, files, directories) = fixture();
    let guard = Guard::acquire(&root.0).unwrap();
    let (staging, journal) = stage(&guard, &source, &files, &directories);
    commit(&journal, &staging).unwrap();
    finish(&staging, b"committed").unwrap();
    fs::write(source.join("New/image.bin"), b"later user edit").unwrap();
    guard.check_clear(&source).unwrap();
    assert_eq!(
        fs::read(source.join("New/image.bin")).unwrap(),
        b"later user edit"
    );
    assert!(!staging.exists());
}

#[test]
fn committed_cleanup_resumes_after_some_payloads_have_already_been_removed() {
    let (root, source, files, directories) = fixture();
    let guard = Guard::acquire(&root.0).unwrap();
    let (staging, journal) = stage(&guard, &source, &files, &directories);
    commit(&journal, &staging).unwrap();
    finish(&staging, b"committed").unwrap();
    fs::remove_file(staging.join("0.before")).unwrap();
    fs::remove_file(staging.join("2.after")).unwrap();
    assert!(guard.recovery_status(&source).unwrap().committed);
    guard.check_clear(&source).unwrap();
    assert!(!staging.exists());
    assert_eq!(
        fs::read(source.join("New/image.bin")).unwrap(),
        [0, 255, 17, 9]
    );
}

#[test]
fn foreign_file_in_a_new_directory_blocks_recovery_before_any_restore() {
    let (root, source, files, directories) = fixture();
    let guard = Guard::acquire(&root.0).unwrap();
    let (staging, journal) = stage(&guard, &source, &files, &directories);
    commit(&journal, &staging).unwrap();
    fs::write(source.join("New/Empty/user.txt"), "owned by user").unwrap();
    let status = guard.recovery_status(&source).unwrap();
    assert_eq!(status.conflicts, [PathBuf::from("New/Empty/user.txt")]);
    assert!(matches!(
        guard.recover(&source, status.snapshot.as_deref().unwrap()),
        Err(EditError::Conflict)
    ));
    assert_eq!(
        fs::read(source.join("Root.xml")).unwrap(),
        files[0].after.as_ref().unwrap().as_slice()
    );
    assert_eq!(
        fs::read_to_string(source.join("New/Empty/user.txt")).unwrap(),
        "owned by user"
    );
}
