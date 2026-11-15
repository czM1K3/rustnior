//! Folder scanning against the real `tests/fixtures` directory.

use std::path::PathBuf;

use rustnior_core::{Collection, ScanOptions};

fn fixtures() -> PathBuf {
    std::path::absolute(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures"))
        .expect("absolute")
}

#[test]
fn single_file_opens_its_folder_at_that_file() {
    let file = fixtures().join("rgb.png");
    let c = Collection::from_paths(std::slice::from_ref(&file), &ScanOptions::default());
    assert!(c.len() >= 10, "found {}", c.len());
    assert_eq!(c.current().map(|e| &e.path), Some(&file));
    let names: Vec<&str> = c.entries().iter().map(|e| e.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| rustnior_core::natsort::natural_cmp_total(a, b));
    assert_eq!(names, sorted);
}

#[test]
fn folder_opens_at_first_image() {
    let c = Collection::from_paths(&[fixtures()], &ScanOptions::default());
    assert_eq!(c.current_index(), Some(0));
}

#[test]
fn several_files_limit_the_collection() {
    let a = fixtures().join("rgb.jpg");
    let b = fixtures().join("alpha.png");
    let c = Collection::from_paths(&[a.clone(), b], &ScanOptions::default());
    assert_eq!(c.len(), 2);
    assert_eq!(c.current().map(|e| &e.path), Some(&a));
}

#[test]
fn missing_path_gives_empty_collection() {
    let c = Collection::from_paths(
        &[fixtures().join("nope"), fixtures().join("nope2")],
        &ScanOptions::default(),
    );
    assert!(c.is_empty());
    assert!(c.current().is_none());
}
