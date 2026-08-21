//! How much of the legacy corpus the M1 parser can consume.
//!
//! Failure is expected and is not a gate: the M1 subset excludes traits,
//! objects, generics, arrays, `for`, and most of the language. The number is
//! tracked so the next milestone can be aimed at whatever actually blocks it.

// clippy.toml's allow-*-in-tests only reaches `#[cfg(test)]` modules.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// `examples/` at the repository ROOT is hand-written demo code rather than
/// corpus. It is skipped by path and not by name: `SpecData/examples` IS
/// corpus, and skipping the name took 137 legacy files out of the metric.
fn is_demo_directory(root: &Path, path: &Path) -> bool {
    path == root.join("examples")
}

fn collect_sources_from(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
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
            if is_demo_directory(root, &path) {
                continue;
            }
            collect_sources_from(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "fss" || e == "fsi") {
            out.push(path);
        }
    }
}

#[test]
fn parses_what_it_can_of_the_corpus_without_panicking() {
    let mut files = Vec::new();
    let root = repo_root();
    collect_sources_from(&root, &root, &mut files);
    files.sort();
    assert!(
        files.len() > 1800,
        "expected the legacy corpus, found {}",
        files.len()
    );

    let mut lexed = 0usize;
    let mut parsed = 0usize;
    let mut blockers: BTreeMap<String, usize> = BTreeMap::new();

    for path in &files {
        let Ok(bytes) = fs::read(path) else { continue };
        let Ok(source) = String::from_utf8(bytes) else {
            continue;
        };
        let Ok(tokens) = fortress_lexer::lex(&source) else {
            continue;
        };
        lexed += 1;
        match fortress_parser::parse(&tokens) {
            Ok(_) => parsed += 1,
            Err(e) => {
                let label = match &e {
                    fortress_parser::ParseError::UnexpectedToken { expected, .. } => {
                        format!("expected {expected}")
                    }
                    fortress_parser::ParseError::UnexpectedEndOfInput { expected } => {
                        format!("eof, expected {expected}")
                    }
                    fortress_parser::ParseError::PostfixOperatorUnsupported { .. } => {
                        "postfix operator".to_owned()
                    }
                    fortress_parser::ParseError::ReservedWord { word, .. } => {
                        format!("reserved word `{word}`")
                    }
                    fortress_parser::ParseError::StaticParameterKindUnsupported {
                        kind, ..
                    } => {
                        format!("`{kind}` static parameter")
                    }
                    fortress_parser::ParseError::LocalFunctionDeclarationUnsupported { .. } => {
                        "local function declaration".to_owned()
                    }
                    fortress_parser::ParseError::ChainedOperatorsDiffer { .. } => {
                        "chain mixes ordering senses".to_owned()
                    }
                    fortress_parser::ParseError::ObjectVarargsParameter { .. } => {
                        "object varargs parameter without `transient`".to_owned()
                    }
                };
                *blockers.entry(label).or_default() += 1;
            }
        }
    }

    eprintln!("\ncorpus: {} files, {lexed} lex cleanly", files.len());
    eprintln!(
        "  parsed {parsed} ({:.1}% of those that lex)",
        (parsed as f64 / lexed as f64) * 100.0
    );

    let mut ranked: Vec<(String, usize)> = blockers.into_iter().collect();
    ranked.sort_by_key(|&(_, count)| core::cmp::Reverse(count));
    eprintln!("\n  what blocks the parser first, top 10:");
    for (label, count) in ranked.iter().take(10) {
        eprintln!("    {count:5}  {label}");
    }

    // The same ratchet. The lexer pass took this from 84 to 154 by adding
    // `import`, the headerless-file production and the tokens above it; M3d's
    // static parameters took it to 168; M3e's `()` took it to 428, of which the
    // unit type alone was 232 and tuples and arrows together were 28. M3f's `=`
    // as an equality operator took it to 477, and M3f's chain sense check gave
    // one back: XXXchain1.fss is the legacy suite's negative test for that rule
    // and its own source says (* SHOULD NOT PARSE *).
    // M3h's bundle -- getter/setter, `self` parameters and component-level
    // value declarations -- took it 476 -> 614. The three were spiked
    // separately first and measured +35, +36 and +53; together they are +138,
    // because a file blocked on one usually contains another.
    // M3k's `^` took it 614 -> 625, and it is the only part of that milestone
    // that moves this number: AND, OR and NOT already parsed as identifiers
    // and died in the checker, where `^` died here.
    // M4's `for` production took it 625 -> 637. `for` was one of the 66
    // reserved words the lexer keeps out of the identifier namespace, and it is
    // intercepted in the parser rather than given a token, so no file in the
    // corpus lexes differently than it did.
    // The floor then went unratcheted through M5, the `opr` spike and M6's
    // declaration modifiers, which between them took the real number to 732.
    // SPIKE-VARARGS took it 732 -> 749: `...` after a parameter type, static
    // parameters between an enclosing operator's opener and its operand, an
    // encloser with no operand at all, and a closing half that need not match
    // the opening half in length.
    assert!(
        parsed >= 749,
        "parser corpus regressed: {parsed} files parse, floor is 749"
    );
}
