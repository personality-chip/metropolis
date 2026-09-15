use lexopt::{Arg, Parser};

#[derive(Debug, Default)]
pub struct Args {
    pub theme: Option<String>,
    pub weather: Option<String>,
    pub debug: bool,
    pub distro: Option<String>,
    pub apps: bool,
    pub snapshot: bool,
    pub samples: Option<usize>,
}

pub fn parse() -> Result<Args, lexopt::Error> {
    let mut args = Args::default();
    let mut parser = Parser::from_env();

    while let Some(arg) = parser.next()? {
        match arg {
            Arg::Long("apps") => args.apps = true,
            Arg::Long("snapshot") => args.snapshot = true,
            Arg::Long("samples") => {
                let value = parser.value()?.into_string()?;
                let n = value.parse::<usize>().map_err(|_| lexopt::Error::from("--samples expects an integer"))?;
                if !(1..=3600).contains(&n) { return Err("--samples must be between 1 and 3600".into()); }
                args.samples = Some(n);
            }
            Arg::Short('t') | Arg::Long("theme") => {
                args.theme = Some(parser.value()?.into_string()?);
            }
            Arg::Short('w') | Arg::Long("weather") => {
                args.weather = Some(parser.value()?.into_string()?);
            }
            Arg::Short('d') | Arg::Long("debug") => {
                args.debug = true;
            }
            Arg::Long("distro") => {
                args.distro = Some(parser.value()?.into_string()?);
            }
            Arg::Short('h') | Arg::Long("help") => {
                print_help();
                std::process::exit(0);
            }
            Arg::Short('v') | Arg::Long("version") => {
                println!("metropolis {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            _ => return Err(arg.unexpected().into()),
        }
    }

    Ok(args)
}

fn print_help() {
    let logo = r#"
  __  __   ______  _______  _____    ____   _____    ____   _       _____   _____ 
 |  \/  | |  ____||__   __||  __ \  / __ \ |  __ \  / __ \ | |     |_   _| / ____|
 | \  / | | |__      | |   | |__) || |  | || |__) || |  | || |       | |  | (___  
 | |\/| | |  __|     | |   |  _  / | |  | ||  ___/ | |  | || |       | |   \___ \ 
 | |  | | | |____    | |   | | \ \ | |__| || |     | |__| || |___   _| |_  ____) |
 |_|  |_| |______|   |_|   |_|  \_\ \____/ |_|      \____/ |______||_____||_____/ 
    "#;

    println!("{}", logo);
    println!("The cyberpunk city powered by your kernel metrics.\n");
    println!("USAGE:");
    println!("  metropolis [OPTIONS]\n");
    println!("OPTIONS:");
    println!("  --apps               Kernel City: one building per application");
    println!("  --snapshot           Print real application metrics as TOML; no terminal UI");
    println!("  --samples <N>        Snapshot count (1..3600, default 1), one per second");
    println!("  -t, --theme <NAME>    Override the global theme (e.g., dracula, cyberpunk)");
    println!("  -w, --weather <MODE>  Override the weather (rain, snow, clear)");
    println!("  --distro <NAME>       Override the detected distribution logo");
    println!("  -d, --debug           Enable debug/diagnostic overlay");
    println!("  -h, --help            Print help information");
    println!("  -v, --version         Print version information");
    println!("\nENVIRONMENT:");
    println!("  Config:  ~/.config/metropolis/config.toml");
    println!("  Themes:  ~/.config/metropolis/themes/");
}
