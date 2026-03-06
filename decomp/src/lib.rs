use std::iter;

use ppc32::decoder::DecodeError;

use crate::{
    ast::{build::AstBuildParams, write::WriteContext},
    dataflow::{
        Instructions,
        ssa::{LocalGenerationAnalysis, def_use_map},
        variables::infer_variables,
    },
    decoder::Decoder,
};

pub mod ast;
pub mod dataflow;
pub mod decoder;

pub fn decompile_into_ast_writer(
    decoder: &mut Decoder<'_>,
    writer: &mut impl ast::write::Writer,
) -> Result<(), DecodeError> {
    let fn_address = decoder.address().0;
    let insts: Instructions = iter::from_fn(|| decoder.next_instruction_with_offset().transpose())
        .collect::<Result<_, _>>()?;

    let analysis = LocalGenerationAnalysis {
        insts: &insts,
        fn_address,
    };
    let local_generations = dataflow::core::run(&analysis);

    let def_use_map = def_use_map(&analysis, &local_generations);

    let variables = infer_variables(&local_generations, &analysis, &def_use_map);

    let ast = ast::build(AstBuildParams {
        fn_address,
        instructions: &insts,
        local_generations: &local_generations,
        analysis: &analysis,
        def_use_map: &def_use_map,
        variables: &variables,
    });

    ast::write::write_ast(
        &ast,
        &WriteContext {
            variables: &variables,
        },
        writer,
    );
    Ok(())
}
