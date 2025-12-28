use std::fs;

use anyhow::{Context, anyhow, bail, ensure};
use dol::Dol;

use crate::args::Args;

mod args;
mod trace;

fn print_headers(dol: &Dol) -> anyhow::Result<()> {
    println!("BSS address: {:x}", dol.bss_address());
    println!("BSS size: {:x}", dol.bss_size());
    println!("Entrypoint: {:x}", dol.entrypoint());

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let Args {
        input,
        addr,
        entrypoint,
        trace,
        headers,
    } = Args::parse()?;

    let dol = Dol::new(fs::read(input).context("failed to read input file")?)
        .map_err(|err| anyhow!("dol validation failed: {err}"))?;

    let addr = || {
        if let Some(addr) = addr {
            ensure!(
                entrypoint == false,
                "cannot provide both -x and --entrypoint"
            );
            Ok(addr)
        } else if entrypoint {
            Ok(dol.entrypoint())
        } else {
            bail!("either -x <address> or --entrypoint must be provided");
        }
    };

    let mut did_anything = false;

    if headers {
        print_headers(&dol)?;
        did_anything = true;
    }

    if trace {
        trace::trace(&dol, addr()?)?;
        did_anything = true;
    }

    if !did_anything {
        eprintln!("No action specified!");
    }

    Ok(())
}
