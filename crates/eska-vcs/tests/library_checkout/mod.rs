//! A real local Git server proves ref selection; checkout itself belongs to gix.

use super::*;

fn checkout(root: &Path, remote: &Path, destination: &Path, locale: &str) -> Output {
    tool(
        root,
        &std::env::var_os("PATH").unwrap_or_default(),
        locale,
        &[
            "__bridge",
            "library-checkout",
            remote.to_str().unwrap(),
            "--destination",
            destination.to_str().unwrap(),
        ],
    )
}

#[test]
fn master_snapshot_is_shallow_exact_and_needs_no_eska_manifest() {
    let fixture = TestDir::new();
    let remote = fixture.0.join("Исходники БСП");
    fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--initial-branch=master", "--template="]);
    fs::create_dir_all(remote.join("src/cf")).unwrap();
    fs::write(remote.join("LICENSE"), "vendor license\r\n").unwrap();
    fs::write(remote.join(".gitignore"), "vendor ignored\n").unwrap();
    fs::write(
        remote.join("src/cf/Configuration.xml"),
        "raw Designer XML\r\n",
    )
    .unwrap();
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-m", "first"]);
    fs::write(remote.join("src/cf/Module.bsl"), "current master\r\n").unwrap();
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-m", "current master"]);
    let expected = String::from_utf8(git(&remote, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    git(&remote, &["tag", "vendor-tag"]);
    git(&remote, &["checkout", "-b", "main"]);
    fs::write(remote.join("main-only"), "not the chosen branch").unwrap();
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-m", "default main"]);
    let mut replies = Vec::new();
    for locale in ["ru", "en"] {
        let target = fixture.0.join(format!("checkout-{locale}"));
        let reply = json(&checkout(&fixture.0, &remote, &target, locale));
        assert_eq!(reply["operation"], "library_checkout");
        assert_eq!(reply["data"]["revision"], expected);
        assert_eq!(reply["data"]["reference"], "refs/heads/master");
        for file in [
            "LICENSE",
            ".gitignore",
            "src/cf/Configuration.xml",
            "src/cf/Module.bsl",
        ] {
            assert_eq!(
                fs::read(target.join(file)).unwrap(),
                fs::read(remote.join(file)).unwrap()
            );
        }
        assert!(!target.join("eska.toml").exists());
        assert!(!target.join("main-only").exists());
        assert_eq!(git(&target, &["rev-list", "--count", "HEAD"]), b"1\n");
        assert_eq!(git(&target, &["tag", "--list"]), [] as [u8; 0]);
        replies.push(reply);
    }
    assert_eq!(replies[0], replies[1]);
    assert!(!remote.join("eska.toml").exists());
}

#[test]
fn failed_master_checkout_removes_only_its_own_new_destination() {
    let fixture = TestDir::new();
    let remote = fixture.0.join("only-main");
    fs::create_dir(&remote).unwrap();
    git(&remote, &["init", "--initial-branch=main", "--template="]);
    fs::write(remote.join("README"), "no master branch").unwrap();
    git(&remote, &["add", "."]);
    git(&remote, &["commit", "-m", "main only"]);
    let target = fixture.0.join("new-checkout");
    let output = checkout(&fixture.0, &remote, &target, "en");
    assert_eq!(output.status.code(), Some(1));
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["error"]["reason"], "network");
    assert!(!target.exists());
    fs::create_dir(&target).unwrap();
    fs::write(target.join("mine"), "user content").unwrap();
    let output = checkout(&fixture.0, &remote, &target, "ru");
    assert_eq!(output.status.code(), Some(1));
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["error"]["reason"], "destination_exists");
    assert_eq!(fs::read(target.join("mine")).unwrap(), b"user content");
    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
}
