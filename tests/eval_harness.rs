//! The recall evaluation's own tests: `eval/test_trail_eval.py`, run here so `cargo test`
//! covers the harness that judges the ranking. It needs `python3` (stdlib only); a machine
//! without one fails here rather than reporting a skipped test as passed.

use std::process::Command;

#[test]
fn the_evaluation_harness_passes_its_own_tests() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/eval");
    let out = Command::new("python3")
        .args(["-I", "test_trail_eval.py"])
        .current_dir(dir)
        .output()
        .expect("python3 runs the evaluation harness's tests");
    assert!(
        out.status.success(),
        "eval/test_trail_eval.py failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
