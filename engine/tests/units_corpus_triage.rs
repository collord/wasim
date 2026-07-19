//! B5 corpus strict-mode triage: run `check_dimensions` over every corpus model and print the
//! dimensional inconsistencies it reports, so they can be classified (real modeling bug → emit
//! doc; checker gap → fix) before strict mode ever becomes the default. Ignored by default (it's
//! a diagnostic, not a pass/fail gate); run with `--ignored --nocapture`.

use std::path::PathBuf;

use wasim_engine::{parse_v2, units};

#[test]
#[ignore]
fn corpus_strict_triage() {
    let dir = PathBuf::from(std::env::var("HOME").unwrap()).join("openvsim/wasim/schema_examples");
    if !dir.exists() {
        eprintln!("skipping: corpus not present");
        return;
    }
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();

    let mut total_models = 0;
    let mut models_with_errors = 0;
    let mut total_errors = 0;
    for p in &files {
        let json = std::fs::read_to_string(p).unwrap();
        let Ok(m) = parse_v2(&json) else { continue };
        total_models += 1;
        let errs = units::check_dimensions(&m);
        if !errs.is_empty() {
            models_with_errors += 1;
            total_errors += errs.len();
            eprintln!("\n=== {} ({} error(s)) ===", p.file_name().unwrap().to_string_lossy(), errs.len());
            for e in errs.iter().take(8) {
                eprintln!("  {e}");
            }
            if errs.len() > 8 {
                eprintln!("  … and {} more", errs.len() - 8);
            }
        }
    }
    eprintln!(
        "\n── TRIAGE SUMMARY ──\n{total_models} models checked, {models_with_errors} with dimensional \
         errors, {total_errors} errors total."
    );
}
