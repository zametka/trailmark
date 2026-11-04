use std::io::{self, Read};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let read_stdin = args.windows(2).any(|w| w[0] == "--body" && w[1] == "-");
    let mut stdin = String::new();
    if read_stdin {
        let _ = io::stdin().read_to_string(&mut stdin);
    }
    let out = trailmark::run(&args, &stdin);
    print!("{}", out.stdout);
    eprint!("{}", out.stderr);
    std::process::exit(out.code);
}
