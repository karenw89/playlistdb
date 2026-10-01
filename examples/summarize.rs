use playlistdb::{OutputFormat, Playlist, Summary};
use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "usage: summarize [--format=text|json|json-pretty] [file]\n\
                     reads the playlist from stdin when no file is given, or when file is -";

fn parse_format(name: &str) -> Option<OutputFormat> {
    match name {
        "text" => Some(OutputFormat::Human),
        "json" => Some(OutputFormat::Json),
        "json-pretty" => Some(OutputFormat::JsonPretty),
        _ => None,
    }
}

fn main() -> ExitCode {
    let mut format = OutputFormat::Human;
    let mut input: Option<String> = None;

    for arg in std::env::args().skip(1) {
        if arg == "-h" || arg == "--help" {
            println!("{}", USAGE);
            return ExitCode::SUCCESS;
        }
        if let Some(name) = arg.strip_prefix("--format=") {
            match parse_format(name) {
                Some(f) => format = f,
                None => {
                    eprintln!("unknown format '{}'\n{}", name, USAGE);
                    return ExitCode::from(2);
                }
            }
        } else if arg != "-" && arg.starts_with('-') {
            eprintln!("unknown option '{}'\n{}", arg, USAGE);
            return ExitCode::from(2);
        } else if input.is_some() {
            eprintln!("only one input file is supported\n{}", USAGE);
            return ExitCode::from(2);
        } else {
            input = Some(arg);
        }
    }

    // A bare "-" means stdin, same as giving no file at all.
    let path = input.filter(|p| p != "-");
    let (name, contents) = match &path {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(c) => (p.clone(), c),
            Err(e) => {
                eprintln!("could not read {}: {}", p, e);
                return ExitCode::FAILURE;
            }
        },
        None => {
            let mut buf = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                eprintln!("could not read stdin: {}", e);
                return ExitCode::FAILURE;
            }
            ("stdin".to_string(), buf)
        }
    };

    let playlist = Playlist::parse_auto(name, &contents);
    println!("{}", Summary::new(&playlist).render(format));
    ExitCode::SUCCESS
}
