use onenote_parser::Parser;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let config = match Config::from_args(env::args()) {
        Ok(config) => config,
        Err(err) => {
            print_usage(&err.program_name, Some(err.reason));
            return ExitCode::from(1);
        }
    };

    eprintln!("Reading {}", config.input_file.display());
    let data = match std::fs::read(&config.input_file) {
        Ok(data) => data,
        Err(err) => {
            print_usage(
                &config.program_name,
                Some(&format!("file read error: {err}")),
            );
            return ExitCode::from(2);
        }
    };

    let parser = Parser::new();
    let input_typed_path = {
        let s = config.input_file.to_string_lossy();
        typed_path::TypedPath::derive(s.as_ref()).to_path_buf()
    };
    let output = match config.mode {
        OutputMode::Section => parser
            .parse_section_buffer(&data, input_typed_path.to_path())
            .map(|section| format!("{:#?}", section)),
        OutputMode::OneStore => parser.dump_onestore(&data),
    };

    match output {
        Ok(text) => {
            println!("{}", text);
            ExitCode::SUCCESS
        }
        Err(err) => {
            print_usage(&config.program_name, Some(&format!("parse error: {err}")));
            ExitCode::from(3)
        }
    }
}

#[derive(PartialEq)]
enum OutputMode {
    Section,
    OneStore,
}

struct Config {
    program_name: String,
    input_file: PathBuf,
    mode: OutputMode,
}

struct ConfigParseError {
    program_name: String,
    reason: &'static str,
}

impl Config {
    fn from_args(args: impl Iterator<Item = String>) -> Result<Self, ConfigParseError> {
        let mut args = args;
        let program_name = args.next().unwrap_or_else(|| "inspect".into());
        let Some(input_file) = args.next() else {
            return Err(ConfigParseError {
                program_name,
                reason: "missing input file",
            });
        };
        let mode = args.next().unwrap_or_else(|| "--onestore".into());
        let mode = match mode.as_str() {
            "--onestore" => OutputMode::OneStore,
            "--section" => OutputMode::Section,
            _ => {
                return Err(ConfigParseError {
                    program_name,
                    reason: "invalid mode (expected --onestore or --section)",
                });
            }
        };
        if args.next().is_some() {
            return Err(ConfigParseError {
                program_name,
                reason: "too many arguments",
            });
        }
        Ok(Config {
            program_name,
            input_file: input_file.into(),
            mode,
        })
    }
}

fn print_usage(program_name: &str, error: Option<&str>) {
    eprintln!("Usage: {program_name} <input_file> [--onestore|--section]");
    eprintln!("       Dumps debug information about a .one / .onetoc2 file.");
    eprintln!("       Output format is unstable and intended for human inspection.");
    if let Some(error) = error {
        eprintln!("error: {error}");
    }
}
