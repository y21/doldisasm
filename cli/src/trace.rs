use crate::{
    args::{AddrRange, AddrRangeEnd},
    decoder::Decoder,
};
use anyhow::Context;
use dol::Dol;

pub fn trace(dol: &Dol, start_addr: AddrRange) -> anyhow::Result<()> {
    let mut queue = vec![start_addr];

    while let Some(address @ AddrRange(start_addr, _)) = queue.pop() {
        let buffer = dol
            .slice_from_load_addr(start_addr)
            .with_context(|| format!("address {start_addr:#x} is not part of any section"))?;

        let mut decoder = Decoder::new(buffer, address);

        let mut jumps = Vec::new();
        let mut last_addr = start_addr;

        loop {
            match decoder.next_instruction_with_offset() {
                Ok(Some((ins_addr, ins))) => {
                    last_addr = ins_addr;

                    println!("{ins_addr:#x} {ins:?}");

                    if let Some(target) = ins.branch_target(ins_addr) {
                        jumps.push(target);
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    eprintln!("(stopping due to decoder error: {err:#x?})");
                    break;
                }
            }
        }

        for jump in jumps {
            if !(start_addr..=last_addr).contains(&jump) {
                queue.push(AddrRange(jump, AddrRangeEnd::Unbounded));
            }
        }
    }

    Ok(())
}
