use std::{
    fs::File,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use bmp::raw::Bmp;

pub(crate) fn bmpsuite_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("bmpsuite")
}

fn suite_is_generated() -> bool {
    let root = bmpsuite_root();
    root.join("stamp").is_file() && ["g", "q", "b", "x"].into_iter().all(|dir| root.join(dir).is_dir())
}

fn ensure_suite_generated() -> bool {
    static GENERATED: OnceLock<bool> = OnceLock::new();
    *GENERATED.get_or_init(|| {
        if suite_is_generated() {
            return true;
        }

        let status = Command::new("make")
            .arg("-C")
            .arg(bmpsuite_root())
            .arg("stamp")
            .status();

        match status {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    })
}

pub(crate) fn require_suite_generated() {
    assert!(
        ensure_suite_generated(),
        "failed to generate bmpsuite fixtures; make sure 'make' and a C compiler are available"
    );
}

pub(crate) fn parse_bmp(path: &Path) -> Result<Bmp, bmp::raw::BmpError> {
    let mut file = File::open(path).unwrap_or_else(|err| panic!("failed to open {}: {err}", path.display()));
    Bmp::read_checked(&mut file)
}

pub(crate) fn to_rel_suite_path(path: &Path) -> String {
    let root = bmpsuite_root();
    path.strip_prefix(&root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
