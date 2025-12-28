use pico_args::Arguments;
use std::{path::PathBuf, str::FromStr};

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
    addr("-x"): Option<u32> = |s| u32::from_str_radix(s.trim_start_matches("0x"), 16),
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
