use anyhow::Context;
use pico_args::Arguments;
use std::{num::ParseIntError, path::PathBuf, str::FromStr};

macro_rules! define_args {
    (
        $(
            $name:ident ( $flag:expr ) $($required_exists:ident)* : $ty:ty $( = $parser:expr )?
        ),*
    ) => {
        pub struct Args {
            $(pub $name: $ty),*
        }
        impl Args {
            pub fn parse() -> anyhow::Result<Self> {
                let mut args = Arguments::from_env();

                macro_rules! parse {
                    (required; $fflag:expr; $pparser:expr) => { args.value_from_fn($fflag, $pparser)? };
                    (required; $fflag:expr;) => { args.value_from_str($fflag)? };
                    (exists; $fflag:expr;) => { args.contains($fflag) };
                    (; $fflag:expr; $pparser:expr) => { args.opt_value_from_fn($fflag, $pparser)? };
                    (; $fflag:expr;) => { args.opt_value_from_str($fflag)? };
                }

                $(
                    let $name = parse!($($required_exists)?; $flag; $($parser)?);
                )*

                Ok(Self {
                    $($name),*
                })
            }
        }
    };
}

define_args! {
    input("-i") required: PathBuf,
    addr("-x"): Option<AddrRange> = parse_addr_range,
    entrypoint("--entrypoint") exists: bool,
    trace("--trace") exists: bool,
    headers("--headers") exists: bool,
    sections("--sections") exists: bool,
    disasm("--disasm"): Option<DisassemblyLanguage> = DisassemblyLanguage::from_str
}

#[derive(Debug)]
pub enum DisassemblyLanguage {
    Asm,
    C,
}

impl FromStr for DisassemblyLanguage {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "asm" => Ok(DisassemblyLanguage::Asm),
            "c" => Ok(DisassemblyLanguage::C),
            _ => Err(anyhow::anyhow!("invalid disassembly language: {}", s)),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum AddrRangeEnd {
    Unbounded,
    Bounded(u32),
}

#[derive(Debug, Copy, Clone)]
pub struct AddrRange(pub u32, pub AddrRangeEnd);

fn parse_addr_range(source: &str) -> anyhow::Result<AddrRange> {
    fn parse_hex(s: &str) -> Result<u32, ParseIntError> {
        u32::from_str_radix(s.trim_start_matches("0x"), 16)
    }

    let (start, end) = source
        .split_once(':')
        .context("invalid address range format, expected -x <start>:<end?> (end is optional)")?;

    let start = parse_hex(start).context("failed to parse start address")?;
    let end = if end.is_empty() {
        AddrRangeEnd::Unbounded
    } else {
        let end = if let Some(rest) = end.strip_prefix('+') {
            let relative: u32 = rest
                .parse()
                .context("failed to parse relative end address")?;
            start + relative
        } else {
            parse_hex(end).context("failed to parse end address")?
        };

        AddrRangeEnd::Bounded(end)
    };

    Ok(AddrRange(start, end))
}
