use std::process::ExitCode;

/// A diagnostic against the user's source.
const EXIT_USER_ERROR: u8 = 1;
/// A compiler bug: malformed IR, a failed linker, an internal invariant broken.
const EXIT_INTERNAL_ERROR: u8 = 70;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.as_slice() {
        [] => {
            eprintln!("usage: fortressc <source.fss> [-o <output>]");
            ExitCode::from(EXIT_USER_ERROR)
        }
        _ => {
            eprintln!("fortressc: the compilation pipeline is not wired up yet");
            ExitCode::from(EXIT_INTERNAL_ERROR)
        }
    }
}
