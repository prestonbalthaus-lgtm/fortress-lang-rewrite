//! Points the lexer at every Fortress source file in the legacy tree.
//!
//! `Err` is a PASSING outcome. The M1 subset cannot lex radix numerals,
//! character literals or non-ASCII operators, all of which are live in the
//! shipped library. The criterion is that the lexer terminates and does not
//! panic. The `Ok` rate is tracked as an informational metric, not a gate.

// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules; an
// integration test is its own crate, so the workspace denies apply here and a
// failing assertion could not panic. Test code is exempt on purpose.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use fortress_lexer::lex;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                // `fortressc` holds our own fixtures; this metric is about the
                // legacy tree only.
                .is_some_and(|n| n == ".git" || n == "target" || n == "fortressc")
            {
                continue;
            }
            collect_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "fss" || e == "fsi") {
            out.push(path);
        }
    }
}

#[test]
fn lexes_the_whole_corpus_without_panicking() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_sources(&root, &mut files);
    files.sort();

    assert!(
        files.len() > 1800,
        "expected the legacy corpus under {}, found {} files",
        root.display(),
        files.len()
    );

    let mut ok = 0usize;
    let mut not_utf8 = 0usize;
    let mut by_error: BTreeMap<String, usize> = BTreeMap::new();
    let mut files_with_non_ascii = 0usize;
    let mut blocking_char: BTreeMap<char, usize> = BTreeMap::new();

    for path in &files {
        let Ok(bytes) = fs::read(path) else { continue };
        let Ok(source) = String::from_utf8(bytes) else {
            not_utf8 += 1;
            continue;
        };
        if !source.is_ascii() {
            files_with_non_ascii += 1;
        }
        match lex(&source) {
            Ok(_) => ok += 1,
            Err(e) => {
                *by_error.entry(format!("{:?}", e.kind)).or_default() += 1;
                // Which character actually stopped us. Fail-fast means only the
                // first one per file is visible, so this is "what blocks M1
                // soonest", not a frequency count over the corpus.
                if let Some(c) = source.get(e.span.start..).and_then(|s| s.chars().next()) {
                    *blocking_char.entry(c).or_default() += 1;
                }
            }
        }
    }

    let lexed = files.len() - not_utf8;
    let errored: usize = by_error.values().sum();
    let err_rate = (errored as f64 / lexed as f64) * 100.0;

    eprintln!(
        "\ncorpus: {} files, {not_utf8} not valid UTF-8",
        files.len()
    );
    eprintln!("  Ok  {ok:5}  ({:.1}%)", (ok as f64 / lexed as f64) * 100.0);
    eprintln!("  Err {errored:5}  ({err_rate:.1}%)");
    eprintln!("\n  error breakdown (first error per file):");
    for (kind, count) in &by_error {
        eprintln!("    {count:5}  {kind}");
    }

    eprintln!(
        "\n  files containing any non-ASCII character: {files_with_non_ascii} ({:.1}%)",
        (files_with_non_ascii as f64 / lexed as f64) * 100.0
    );
    eprintln!("  (independent scan; unlike the breakdown above this is not masked by fail-fast)");

    let mut blockers: Vec<(char, usize)> = blocking_char.into_iter().collect();
    blockers.sort_by_key(|&(_, count)| core::cmp::Reverse(count));
    eprintln!("\n  characters that stop M1 first, top 12:");
    for (c, count) in blockers.iter().take(12) {
        eprintln!("    {count:5}  {c:?}  (U+{:04X})", *c as u32);
    }

    assert_eq!(
        ok + errored,
        lexed,
        "every readable file must reach a verdict"
    );

    // A ratchet, not a target. The number only ever goes up; a change that drops
    // it is a regression and should fail here rather than be noticed a
    // milestone later. M3d's lexer pass took this from 1277 to 1780 by adding
    // `|`, `<|`, `|>`, `||`, `=>`, `^` and `#`. The floor then went unratcheted
    // through the `opr` spike's backslash, which took the real number to 1807.
    // Moving `equals = "=" (!op)` out of the lexer and into the parser's
    // binding position took it 1807 -> 1810.
    assert!(
        ok >= 1810,
        "lexer corpus regressed: {ok} files lex, floor is 1810"
    );
}
