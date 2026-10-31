use std::{
    fs, io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct TestDir(pub PathBuf);

impl TestDir {
    pub fn new() -> Self {
        let base =
            std::env::var_os("ESKA_TEST_ROOT").map_or_else(std::env::temp_dir, PathBuf::from);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after Unix epoch")
            .as_nanos();
        // Clock resolution is not a uniqueness guarantee across parallel tests.
        // Exclusively claim a path and retry collisions without touching it.
        for attempt in 0..1024 {
            let path = base.join(format!(
                "eska-test-{}-{unique}-{attempt} каталог",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(fs::canonicalize(path).expect("canonical test directory")),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create isolated test directory: {error}"),
            }
        }
        panic!("could not claim an isolated test directory after 1024 attempts");
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let result = fs::remove_dir_all(&self.0);
        #[cfg(windows)]
        let result = {
            let mut result = result;
            // Windows can briefly retain Git or scanner handles after the operation completes.
            for _ in 0..20 {
                if !result
                    .as_ref()
                    .is_err_and(|error| matches!(error.raw_os_error(), Some(5 | 32 | 33 | 145)))
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
                result = fs::remove_dir_all(&self.0);
            }
            result
        };
        if let Err(error) = result {
            if std::thread::panicking() {
                // Preserve the original test failure instead of aborting the entire test executable.
                eprintln!("remove owned test directory {}: {error}", self.0.display());
            } else {
                panic!("remove owned test directory: {error}");
            }
        }
    }
}
