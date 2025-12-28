# `./doldisasm`

A very cool work in progress GCN/Wii DOL disassembler/decompiler.

### features
- [Print DOL headers and sections](#dol-headers-and-sections)
- [Disassemble a function's assembly](#disassemble-a-functions-assembly)
- TODO: [decompile a function and try to output corresponding C code](#decompile-a-function-into-c-code)

### examples
These examples assume a `input.dol` file exists in the directory being the DOL to disassemble, as well as a binary called `doldisasm` obtained by compiling the `cli` Rust package in this repository (run: `cargo b -p cli -r`, the binary can be found in `./target/release/doldisasm`).

#### DOL headers and sections
```
$ ./doldisasm -i input.dol --headers --sections

BSS address: 0x8022a300
BSS size: 0x7de40
Entrypoint: 0x80004000

Section #0: file offset 0x100, load address 0x80004000, size 0x1909c0
Section #7: file offset 0x190ac0, load address 0x801949c0, size 0x95940
(Note: 16 sections with size 0 were omitted)
```

#### disassemble a function's assembly
Let's assume that I know that there's a function to be loaded at 0x80008090 and I want to see its assembly. Use `-x` to specify the start address and `--disasm asm` to output assembly.
`--entrypoint` can be used in place of `-x` to use the DOLs entrypoint.
> **NOTE:** since the end of a function is not very clearly defined in a stream of instructions, this command may very well decode and print past the end of the function. Although it is planned to try to "guess" it later
```
$ ./doldisasm -i input.dol -x 0x80008090 --disasm asm

0x80008090 Stwu { source: Register(1), dest: Register(1), imm: Immediate(-32) }
0x80008094 Mfspr { dest: Register(0), spr: Lr }
0x80008098 Stw { source: Register(29), dest: Register(1), imm: Immediate(20) }
...
0x800080f4 Mtspr { source: Register(0), spr: Lr }
0x800080f8 Lwz { dest: Register(31), source: Register(1), imm: Immediate(28) }
0x800080fc Addi { dest: Register(1), source: Register(1), imm: Immediate(32) }
0x80008100 Bclr { bo: BranchAlways, bi: 0, link: false }
```

<details>

<summary>
Comparison to objdump on the original ELF to check that this output is correct
</summary>

```
$ powerpc-eabi-objdump input.elf --disassemble | grep 80008090 -A100

80008090:       94 21 ff e0     stwu    r1,-32(r1)
80008094:       7c 08 02 a6     mflr    r0
80008098:       93 a1 00 14     stw     r29,20(r1)
...
800080f4:       7c 08 03 a6     mtlr    r0
800080f8:       83 e1 00 1c     lwz     r31,28(r1)
800080fc:       38 21 00 20     addi    r1,r1,32
80008100:       4e 80 00 20     blr
```
(note that objdump displays simplified mnemonics, so even though one says 'mtlr r0' while the other says 'mtspr lr r0', they are still essentially saying the same thing)
</details>


#### decompile a function into C code
This isn't implemented yet but the plan is after parsing the instructions, we try to recognize patterns like backjumps (which are likely loops) and reconstruct those patterns into C code, to get a higher level idea of what a function is doing
