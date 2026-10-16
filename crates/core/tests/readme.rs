//! Keeps the README configuration table in sync with the keys the parser accepts.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use claude_sessions_core::config::KEYS;

#[test]
fn readme_config_table_lists_exactly_the_known_keys() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    let readme = fs::read_to_string(path).unwrap();
    let start = readme
        .find("<!-- config-table:start -->")
        .expect("start marker");
    let end = readme
        .find("<!-- config-table:end -->")
        .expect("end marker");

    let documented: BTreeSet<&str> = readme[start..end]
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|rest| rest.split('`').next())
        .collect();
    let known: BTreeSet<&str> = KEYS.iter().copied().collect();

    assert_eq!(documented, known);
}
