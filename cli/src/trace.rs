use anyhow::Context;
use dol::Dol;
use ppc32::{
    decoder::Decoder,
    instruction::{AddressingMode, Instruction},
};

pub fn trace(dol: &Dol, start_addr: u32) -> anyhow::Result<()> {
    let mut queue = vec![start_addr];

    while let Some(address) = queue.pop() {
        println!("\n--- Decoding {:#x}---", address);

        let section = dol
            .section_of_load_addr(address)
            .context("failed to find section of address")?;

        let file_offset = section.file_offset_of_addr(address);
        let buffer = &dol.as_bytes()[file_offset as usize..];

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
