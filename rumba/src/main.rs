use std::cmp::max;

use clap::{Arg, Command};
use mbalib::{nonpoly::solve_non_poly, parser::parse_expr};

fn main() {
    let matches = Command::new("text-tool")
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
                .value_parser(clap::value_parser!(u32)),
        )
        .get_matches();

    let expr = matches.get_one::<String>("expression").unwrap().to_string();
    let bits = *matches.get_one::<u32>("n").unwrap();

    let mut hex = false;
    if matches.get_flag("hex") {
        hex = true;
    }

    match parse_expr(&expr) {
        Ok(e) => {
            let sol = solve_non_poly(&e, bits);
            println!("{}", sol.repr(bits, hex, false));

            if matches.get_flag("test") {
                let mut count = 0;
                let max_var = max(
                    e.get_vars().iter().copied().max().unwrap_or(0),
                    sol.get_vars().iter().copied().max().unwrap_or(0),
                );

                for _ in 0..1000 {
                    let mut vars = vec![];
                    for _ in 0..=max_var {
                        vars.push(rand::random_range(0..2u128.pow(bits)));
                    }
                    if (e.eval(&vars) % (2u128.pow(bits))) == (sol.eval(&vars) % (2u128.pow(bits)))
                    {
                        count += 1;
                    } else {
                        eprintln!(
                            "v0={} v1 ={} e={} MBA={}",
                            vars[0],
                            vars[1],
                            e.eval(&vars) % (2u128.pow(bits)),
                            sol.eval(&vars) % (2u128.pow(bits))
                        )
                    }
                }

                println!("Tested on 1K values found {} errors", 1000 - count);
            }
        }

        Err(e) => {
            eprintln!("{}", e);
        }
    }
}
