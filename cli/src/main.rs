use std::fs;

use anyhow::{Context, anyhow, bail, ensure};
use dol::Dol;

use crate::args::{AddrRange, AddrRangeEnd, Args};

mod args;
mod decoder;
mod disasm;
mod trace;
mod value;

fn main() -> anyhow::Result<()> {
    let Args {
        input,
        addr,
        entrypoint,
        trace,
        headers,
        sections,
        disasm,
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
            Ok(AddrRange(dol.entrypoint(), AddrRangeEnd::Unbounded))
        } else {
            bail!("either -x <address> or --entrypoint must be provided");
        }
    };

    let mut did_anything = false;

    if headers {
        print_headers(&dol)?;
        did_anything = true;
    }

    if sections {
        print_sections(&dol)?;
        did_anything = true;
    }

    if trace {
        trace::trace(&dol, addr()?)?;
        did_anything = true;
    }

    if let Some(lang) = disasm {
        disasm::disasm(&dol, addr()?, lang)?;
        did_anything = true;
    }

    if !did_anything {
        eprintln!("No action specified!");
    }

    Ok(())
}

fn print_headers(dol: &Dol) -> anyhow::Result<()> {
    println!("BSS address: {:#x}", dol.bss_address());
    println!("BSS size: {:#x}", dol.bss_size());
    println!("Entrypoint: {:#x}\n", dol.entrypoint());

    Ok(())
}

fn print_sections(dol: &Dol) -> anyhow::Result<()> {
    let mut zero_filtered = 0;

    for (i, section) in dol.sections().enumerate() {
        if section.empty() {
            zero_filtered += 1;
        } else {
            println!(
                "Section #{}: file offset {:#x}, load address {:#x}, size {:#x}",
                i, section.file_offset, section.load_offset, section.size
            );
        }
    }

    if zero_filtered > 0 {
        println!(
            "(Note: {} sections with size 0 were omitted)",
            zero_filtered
        );
    }

    Ok(())
}
