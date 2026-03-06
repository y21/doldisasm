use core::panic;
use std::{env, io::ErrorKind, path::PathBuf, process::ExitCode};

use decomp::{
    ast::write::StringWriter,
    decoder::{AddrRange, AddrRangeEnd, Decoder},
};
use glob::Pattern;

struct TestCase {
    name: &'static str,
    code: &'static [u8],
}

macro_rules! test {
    ($name:ident $($byte:literal,)*) => {
        TestCase {
            name: stringify!($name),
            code: &[$($byte),*],
        }
    }
}

fn main() -> ExitCode {
    let should_bless = matches!(env::var("BLESS").as_deref(), Ok("1"));
    let tests = &[
        test!(empty),
        // return
        test!(empty_return 0x4e, 0x80, 0x00, 0x20, // blr
        ),
        // return x + 2;
        test!(add_return
            0x38, 0x63, 0x00, 0x02, // addi r3, r3, 2
            0x4e, 0x80, 0x00, 0x20, // blr
        ),
        // if (x == 3) {
        //     return a();
        // }
        test!(early_return
            0x2c, 0x03, 0x00, 0x03, 	// cmpwi   r3,3
            0x4c, 0xa2, 0x00, 0x20, 	// bnelr+
            0x94, 0x21, 0xff, 0xf8, 	// stwu    r1,-8(r1)
            0x7c, 0x08, 0x02, 0xa6, 	// mflr    r0
            0x90, 0x01, 0x00, 0x0c, 	// stw     r0,12(r1)
            0x48, 0x00, 0x00, 0x15, 	// bl      deadbf18 <test+0x28>
            0x80, 0x01, 0x00, 0x0c, 	// lwz     r0,12(r1)
            0x7c, 0x08, 0x03, 0xa6, 	// mtlr    r0
            0x38, 0x21, 0x00, 0x08, 	// addi    r1,r1,8
            0x4e, 0x80, 0x00, 0x20, 	// blr
        ),
        test!(if_else_calls_refs
            0x94, 0x21, 0xff, 0xe0,      // stwu    r1,-32(r1)
            0x7c, 0x69, 0x1b, 0x78,      // mr      r9,r3
            0x7c, 0x08, 0x02, 0xa6,      // mflr    r0
            0x38, 0x61, 0x00, 0x08,      // addi    r3,r1,8
            0x93, 0xc1, 0x00, 0x18,      // stw     r30,24(r1)
            0x93, 0xe1, 0x00, 0x1c,      // stw     r31,28(r1)
            0x7d, 0x3f, 0x4b, 0x78,      // mr      r31,r9
            0x90, 0x01, 0x00, 0x24,      // stw     r0,36(r1)
            0x48, 0x0b, 0xaa, 0xa1,      // bl      800c0d30 <DI_ReadDiscID>
            0x7c, 0x6a, 0x1b, 0x79,      // mr.     r10,r3
            0x7d, 0x5e, 0x53, 0x78,      // mr      r30,r10
            0x40, 0x82, 0x00, 0x30,      // bne     800062cc <rrc_di_get_disk_id+0x5c>
            0x38, 0x81, 0x00, 0x08,      // addi    r4,r1,8
            0x7f, 0xe3, 0xfb, 0x78,      // mr      r3,r31
            0x38, 0xa0, 0x00, 0x08,      // li      r5,8
            0x48, 0x17, 0x48, 0x6d,      // bl      8017ab18 <memcpy>
            0x80, 0x01, 0x00, 0x24,      // lwz     r0,36(r1)
            0x7f, 0xc3, 0xf3, 0x78,      // mr      r3,r30
            0x83, 0xe1, 0x00, 0x1c,      // lwz     r31,28(r1)
            0x83, 0xc1, 0x00, 0x18,      // lwz     r30,24(r1)
            0x7c, 0x08, 0x03, 0xa6,      // mtlr    r0
            0x38, 0x21, 0x00, 0x20,      // addi    r1,r1,32
            0x4e, 0x80, 0x00, 0x20,      // blr
            0x7f, 0xe3, 0xfb, 0x78,      // mr      r3,r31
            0x38, 0xa0, 0x00, 0x08,      // li      r5,8
            0x38, 0x80, 0x00, 0x00,      // li      r4,0
            0x48, 0x16, 0xb9, 0x81,      // bl      80171c58 <memset>
            0x80, 0x01, 0x00, 0x24,      // lwz     r0,36(r1)
            0x7f, 0xc3, 0xf3, 0x78,      // mr      r3,r30
            0x83, 0xe1, 0x00, 0x1c,      // lwz     r31,28(r1)
            0x83, 0xc1, 0x00, 0x18,      // lwz     r30,24(r1)
            0x7c, 0x08, 0x03, 0xa6,      // mtlr    r0
            0x38, 0x21, 0x00, 0x20,      // addi    r1,r1,32
            0x4e, 0x80, 0x00, 0x20,      // blr
        ),
        // if (x == 3) {
        //     a();
        // } else {
        //     if (x == 4) {
        //         b();
        //     } else {
        //         c();
        //     }
        // }
        // return d(1);
        test!(nested_if_else
            0x94, 0x21, 0xff, 0xf8, 	    // stwu    r1,-8(r1)
            0x7c, 0x08, 0x02, 0xa6, 	    // mflr    r0
            0x90, 0x01, 0x00, 0x0c, 	    // stw     r0,12(r1)
            0x2c, 0x03, 0x00, 0x03, 	    // cmpwi   r3,3
            0x41, 0x82, 0x00, 0x28, 	    // beq     deadbf28 <test+0x38>
            0x2c, 0x03, 0x00, 0x04, 	    // cmpwi   r3,4
            0x41, 0x82, 0x00, 0x28, 	    // beq     deadbf30 <test+0x40>
            0x48, 0x00, 0x00, 0x2d, 	    // bl      deadbf38 <test+0x48>
            0x38, 0x60, 0x00, 0x01, 	    // li      r3,1
            0x48, 0x00, 0x00, 0x35, 	    // bl      deadbf48 <test+0x58>
            0x80, 0x01, 0x00, 0x0c, 	    // lwz     r0,12(r1)
            0x7c, 0x08, 0x03, 0xa6, 	    // mtlr    r0
            0x38, 0x21, 0x00, 0x08, 	    // addi    r1,r1,8
            0x4e, 0x80, 0x00, 0x20, 	    // blr
            0x48, 0x00, 0x00, 0x31, 	    // bl      deadbf58 <test+0x68>
            0x4b, 0xff, 0xff, 0xe4, 	    // b       deadbf10 <test+0x20>
            0x48, 0x00, 0x00, 0x39, 	    // bl      deadbf68 <test+0x78>
            0x4b, 0xff, 0xff, 0xdc, 	    // b       deadbf10 <test+0x20>
        ),
    ];
    let pattern = env::var("PATTERN")
        .map_or_else(|_| Pattern::new("*"), |pat| Pattern::new(&pat))
        .unwrap();

    let mut had_errors = false;

    for test in tests {
        if !pattern.matches(test.name) {
            continue;
        }

        let mut decoder = Decoder::new(
            test.code,
            AddrRange(0, AddrRangeEnd::Bounded(test.code.len() as u32)),
        );
        let mut output = StringWriter::new();
        decomp::decompile_into_ast_writer(&mut decoder, &mut output).unwrap();
        let output = output.into_string();

        let mut path = PathBuf::from("tests/output");
        path.push(test.name);

        if should_bless {
            eprintln!("blessing output for test {}...", test.name);
            std::fs::write(&path, output).unwrap();
        } else {
            match std::fs::read_to_string(&path) {
                Ok(contents) => {
                    if contents == output {
                        eprintln!("test {} passed!", test.name);
                    } else {
                        eprintln!(
                            "test {} failed!\nExpected:\n{contents}\n\nGot:\n{output}",
                            test.name
                        );
                        had_errors = true;
                    }
                }
                Err(err) if err.kind() == ErrorKind::NotFound => {
                    eprintln!(
                        "test {} failed: output file does not exist (run with BLESS=1 to create it)",
                        test.name
                    );
                    had_errors = true;
                }
                Err(err) => panic!("failed to read output file for test {}: {err}", test.name),
            }
        }
    }

    if had_errors {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
