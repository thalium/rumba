use clap::{Arg, Command, Parser, Subcommand, command};
use rumba_core::{parser::parse_expr, simplify::simplify_mba, varint::make_mask};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
}

#[derive(Subcommand, Debug)]
enum Mode {
    /// Use a raw string input
    Program {
        /// The input string
        input: String,
    },

    /// Use a file path
    Expression {
        /// The input file path
        input: PathBuf,
    },
}

fn main() {
    env_logger::init();

    let matches = Command::new("rumba")
        .version("0.1")
        .author("Jack Royer")
        .about("Accidently breaks polynomial MBAs")
        .arg(
            Arg::new("expression")
                .help("Polynomial MBA with variables v0, v1")
                .required(true),
        )
        .arg(
            Arg::new("hex")
                .long("hex")
                .help("Output constants as hex")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("test")
                .long("test")
                .help("Test the simplified MBA")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("n")
                .long("n")
                .help("Number of bits")
                .value_name("uint")
                .default_value("32")
                .value_parser(clap::value_parser!(u8)),
        )
        .get_matches();

    let expr = matches.get_one::<String>("expression").unwrap().to_string();
    let bits = *matches.get_one::<u8>("n").unwrap();
    let mask = make_mask(bits);

    let mut hex = false;
    if matches.get_flag("hex") {
        hex = true;
    }

    match parse_expr(&expr) {
        Ok(e) => {
            println!("Simplify {}", e.repr(bits, mask, hex, false));
            let sol = simplify_mba(e.clone(), bits);
            println!("{}", sol.repr(bits, mask, hex, false));

            if matches.get_flag("test") {
                if let Err((vars, v1, v2)) = e.sem_equal(&sol, mask, 1000) {
                    eprintln!(
                        "v0={} v1 ={} e(v0, v1)={} MBA(v0, v1)={}",
                        vars[0], vars[1], v1, v2
                    )
                } else {
                    println!("Tested on 1K values found no errors");
                }
            }
        }

        Err(e) => {
            eprintln!("{}", e);
        }
    }
}
