use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();

    if args.is_empty() {
        let mut inp = String::new();
        io::stdin().lock().read_to_string(&mut inp)?;
        match nix2js::translate(&inp, "<stdin>") {
            Ok((x, _)) => {
                io::stdout().write_all(x.as_bytes())?;
            }
            Err(xs) => {
                for e in xs {
                    eprintln!("{}", e);
                }
            }
        }
    } else {
        let inpf = args.remove(0);
        if inpf == "--help" {
            println!("USAGE: nix2js [INPUT_FILE [OUTPUT_FILE [OUT_SOURCE_MAP_FILE]]]");
            return Ok(());
        }
        let inp = std::fs::read_to_string(&inpf)?;
        match nix2js::translate(&inp, &inpf) {
            Err(xs) => {
                for e in xs {
                    eprintln!("{}", e);
                }
            }
            Ok((mut js, map)) => {
                if let Some(outpf) = args.get(0) {
                    if let Some(mapf) = args.get(1) {
                        std::fs::write(&mapf, map.as_bytes())?;
                        js += "\n# sourceMappingURL=";
                        js += mapf;
                    }
                    let _ = map;
                    std::fs::write(outpf, js.as_bytes())?;
                } else {
                    io::stdout().write_all(js.as_bytes())?;
                }
            }
        }
    }

    Ok(())
}
