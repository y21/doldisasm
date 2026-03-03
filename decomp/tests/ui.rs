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
        test!(empty_return 0x4e, 0x80, 0x00, 0x20, // blr
        ),
        test!(add_return
            0x38, 0x63, 0x00, 0x02, // addi r3, r3, 2
            0x4e, 0x80, 0x00, 0x20, // blr
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
