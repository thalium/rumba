use clap::{Arg, Command};
use rumba_core::{
    parser::parse_expr,
    simplify::{SimplifyOptions, simplify_mba_with},
};

fn main() {
    env_logger::init();

    let matches = Command::new("rumba")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Jack Royer")
        .about("Simplifies polynomial MBA expressions")
        .arg(
            Arg::new("expression")
                .help("Polynomial MBA over the variables v0, v1, ...")
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
        .arg(
            Arg::new("no-patterns")
                .long("no-patterns")
                .help("Disable the structural pattern-rewrite engine")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let expr = matches.get_one::<String>("expression").unwrap().to_string();
    let bits = *matches.get_one::<u8>("n").unwrap();
    let options = SimplifyOptions {
        patterns: !matches.get_flag("no-patterns"),
    };

    let hex = matches.get_flag("hex");

    match parse_expr(&expr) {
        Ok(e) => {
            println!("Simplify {}", e.repr(bits, hex, false));
            let sol = match simplify_mba_with(e.clone(), bits, options) {
                Ok(solution) => solution,
                Err(error) => {
                    eprintln!("Failed to simplify expression: {error}");
                    return;
                }
            };
            println!("{}", sol.repr(bits, hex, false));

            if matches.get_flag("test") {
                if let Err((vars, got, want)) = e.sem_equal(&sol, bits, 1000) {
                    let assignment = vars
                        .iter()
                        .enumerate()
                        .map(|(i, v)| format!("v{i}={v}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    eprintln!("counterexample: {assignment} input={got} simplified={want}")
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
