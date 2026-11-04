//! One file per piece of reasoning, with budgeted indexes and a context bundle.
//! The tool knows no project until `trail.toml` tells it.

mod adapter;
mod check;
mod config;
mod context;
mod frontmatter;
mod gitinfo;
mod id;
mod indexgen;
mod issue;
mod ledger;
mod mutate;
mod nearby;
mod record;
mod relate;
mod scan;
mod search;
mod store;
mod terms;
mod typehelp;
mod util;

mod cli;

pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(args: &[String], stdin: &str) -> Outcome {
    cli::dispatch(args, stdin)
}
