use std::{fs, path::PathBuf};

use anyhow::{Context, bail};
use pico_args::Arguments;

use ppc32::{
    decoder::Decoder,
    instruction::{AddressingMode, Instruction},
};

#[derive(Debug)]
struct SectionInfo {
    file_offset: u32,
    load_offset: u32,
    size: u32,
}

impl SectionInfo {
    pub fn contains_addr(&self, addr: u32) -> bool {
        addr >= self.load_offset && addr < self.load_offset + self.size
    }

    pub fn file_offset_of_addr(&self, addr: u32) -> u32 {
        assert!(self.contains_addr(addr));
        self.file_offset + (addr - self.load_offset)
    }
}

#[derive(Debug)]
struct Dol(Vec<u8>);

impl Dol {
    /*
    Start 	End 	Length 	Description
    0x0 	0x3 	4 	File offset to start of Text0
    0x04 	0x1b 	24 	File offsets for Text1..6
    0x1c 	0x47 	44 	File offsets for Data0..10
    0x48 	0x4B 	4 	Loading address for Text0
    0x4C 	0x8F 	68 	Loading addresses for Text1..6, Data0..10
    0x90 	0xD7 	72 	Section sizes for Text0..6, Data0..10
    0xD8 	0xDB 	4 	BSS address
    0xDC 	0xDF 	4 	BSS size
    0xE0 	0xE3 	4 	Entry point
    0xE4 	0xFF 		padding
    */
    const BSS_ADDR_OFF: usize = 0xD8;
    const BSS_SIZE_OFF: usize = 0xDC;
    const SECTION_OFFSET_OFF: usize = 0;
    const SECTION_ADDRESS_OFF: usize = 0x48;
    const SECTION_SIZE_OFF: usize = 0x90;
    const ENTRYPOINT_OFF: usize = 0xE0;

    pub fn new(bytes: Vec<u8>) -> anyhow::Result<Self> {
        if bytes.len() < 0xFF {
            bail!(".dol file smaller than 255 bytes (does not contain all headers");
        }
        Ok(Self(bytes))
    }

    fn u32(&self, off: usize) -> u32 {
        let bytes: [u8; 4] = self.0[off..][..4].try_into().unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn section(&self, section: usize) -> SectionInfo {
        assert!(section <= 17);
        SectionInfo {
            file_offset: self.u32(Self::SECTION_OFFSET_OFF + section * 4),
            load_offset: self.u32(Self::SECTION_ADDRESS_OFF + section * 4),
            size: self.u32(Self::SECTION_SIZE_OFF + section * 4),
        }
    }

    pub fn sections(&self) -> impl Iterator<Item = SectionInfo> + '_ {
        (0..18).map(|i| self.section(i))
    }

    pub fn section_of_load_addr(&self, addr: u32) -> Option<SectionInfo> {
        self.sections().find(|s| s.contains_addr(addr))
    }

    pub fn entrypoint(&self) -> u32 {
        self.u32(Self::ENTRYPOINT_OFF)
    }

    pub fn bss_address(&self) -> u32 {
        self.u32(Self::BSS_ADDR_OFF)
    }

    pub fn bss_size(&self) -> u32 {
        self.u32(Self::BSS_SIZE_OFF)
    }
}

fn print_headers(dol: &Dol) -> anyhow::Result<()> {
    println!("BSS address: {:x}", dol.bss_address());
    println!("BSS size: {:x}", dol.bss_size());
    println!("Entrypoint: {:x}", dol.entrypoint());

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = Arguments::from_env();

    let input: PathBuf = args.value_from_str("-i")?;

    let dol = Dol::new(fs::read(input).context("failed to read input file")?)
        .context("dol validation failed")?;

    print_headers(&dol)?;

    let mut queue = vec![
        // dol.entrypoint(), // 0x80004e30,
        0x800079b0,
    ];

    while let Some(address) = queue.pop() {
        println!("\n--- Decoding {:#x}---", address);

        let section = dol
            .section_of_load_addr(address)
            .context("failed to find section of address")?;

        let file_offset = section.file_offset_of_addr(address);
        let buffer = &dol.0[file_offset as usize..];

        let mut decoder = Decoder::new(buffer);

        let mut jumps = Vec::new();

        loop {
            let offset = decoder.offset();
            let instruction_address = address + offset as u32;
            match decoder.decode_instruction() {
                Ok(instruction) => {
                    print!("{instruction_address:#x} {instruction:?}");

                    if let Instruction::Branch {
                        target,
                        mode,
                        link: _,
                    } = instruction
                    {
                        let abs_target = match mode {
                            AddressingMode::Absolute => target as u32,
                            AddressingMode::Relative => (address + offset as u32)
                                .checked_add_signed(target)
                                .unwrap(),
                        };

                        print!(" ({abs_target:#x})");

                        jumps.push(abs_target);
                    }

                    println!();
                }
                Err(err) => {
                    println!("(stopping due to error: {err:#x?})");

                    let end_address = address + offset as u32;

                    for jump in jumps {
                        // Only add the jump if it isn't "part" of this function (i.e. between address and err.offset())
                        if !(address..end_address).contains(&jump) {
                            queue.push(jump);
                        }
                    }
                    break;
                }
            }
        }
    }

    Ok(())
}
