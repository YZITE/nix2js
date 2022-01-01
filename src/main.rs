use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    let mut inp = String::new();

    io::stdin().lock().read_to_string(&mut inp)?;

    match nix2js::translate(&inp) {
        Ok(x) => {
            io::stdout().write_all(x.as_bytes())?;
        }
        Err(xs) => {
            for e in xs {
                eprintln!("{}", e);
            }
        }
    }

    Ok(())
}
