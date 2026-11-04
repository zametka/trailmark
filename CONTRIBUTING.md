# Contributing

Build and test from this directory:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
python3 -I eval/trail_eval.py synthetic target/release/trailmark --random 40
```

`cargo fmt` with no `--check` rewrites the tree. Format the files you edited.

Fixtures and examples are synthetic. Do not add records, queries, or paths from a private project. An id in a test is an id the test minted.

`trailmark adapter install` must not replace an existing `.claude/settings.json`. A change to that behaviour needs a test that the settings bytes are unchanged.

The read guard fails open on input it does not understand. A new block has to be an explicit branch, with a case that is logged and a near miss that is not.

## Release

The publish workflow is `.github/workflows/publish.yml`. It runs when a GitHub release is published, the same way as axum-vite. Pushing a tag does not publish the crate. Publishing the GitHub release does.

The workflow file has to be on the default branch before that release. The release tag is the version in `Cargo.toml`, as in axum-vite (`0.1.1`, or `v0.1.1`). The job checks that the tag and the version match, runs fmt, clippy, and the tests, then `cargo publish --locked`.

The repository secret `CARGO_REGISTRY_TOKEN` is the crates.io API token. It is not stored in the tree.
