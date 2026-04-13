use cranelift::prelude::Configurable;
use cranelift::{
    codegen::{
        Context,
        control::ControlPlane,
        ir::{AbiParam, Function, Signature, UserFuncName, immediates::Offset32, types::I64},
        isa::CallConv,
    },
    prelude::{
        Block, FunctionBuilder, FunctionBuilderContext, InstBuilder, MemFlags, Value,
        isa::{self},
        settings,
    },
};

use target_lexicon::Triple;

use crate::expr::Expr;

type JitFunctionPtr = unsafe extern "C" fn(*const u64) -> u64;

pub struct JitFunction {
    // Buffer owns the memory
    #[allow(dead_code)]
    buffer: memmap2::Mmap,

    ptr: JitFunctionPtr,
}

impl JitFunction {
    pub fn eval(&self, vars: &[u64]) -> u64 {
        unsafe { (self.ptr)(vars.as_ptr()) }
    }
}

fn translate_expr(e: &Expr, builder: &mut FunctionBuilder, block: &Block) -> Value {
    match e {
        Expr::Var(idx) => {
            let arg = builder.block_params(*block)[0];
            let flags = MemFlags::new().with_readonly();
            builder
                .ins()
                .load(I64, flags, arg, Offset32::new(8 * idx.0 as i32))
        }

        Expr::Const(var_int) => builder.ins().iconst(I64, **var_int as i64),

        Expr::Not(expr) => {
            let e = translate_expr(expr.as_ref(), builder, block);
            builder.ins().bnot(e)
        }

        Expr::Scale(var_int, expr) => {
            let e = translate_expr(expr.as_ref(), builder, block);
            builder.ins().imul_imm(e, **var_int as i64)
        }

        Expr::And(exprs) => {
            let mut res = translate_expr(&exprs[0], builder, block);

            for e in &exprs[1..] {
                let v = translate_expr(e, builder, block);
                res = builder.ins().band(res, v);
            }

            res
        }

        Expr::Or(exprs) => {
            let mut res = translate_expr(&exprs[0], builder, block);

            for e in &exprs[1..] {
                let v = translate_expr(e, builder, block);
                res = builder.ins().bor(res, v);
            }

            res
        }

        Expr::Xor(exprs) => {
            let mut res = translate_expr(&exprs[0], builder, block);

            for e in &exprs[1..] {
                let v = translate_expr(e, builder, block);
                res = builder.ins().bxor(res, v);
            }

            res
        }

        Expr::Add(exprs) => {
            let mut res = translate_expr(&exprs[0], builder, block);

            for e in &exprs[1..] {
                let v = translate_expr(e, builder, block);
                res = builder.ins().iadd(res, v);
            }

            res
        }

        Expr::Mul(exprs) => {
            let mut res = translate_expr(&exprs[0], builder, block);

            for e in &exprs[1..] {
                let v = translate_expr(e, builder, block);
                res = builder.ins().imul(res, v);
            }

            res
        }
    }
}

pub fn compile(e: &Expr) -> JitFunction {
    let mut sig = Signature::new(CallConv::SystemV);

    // Args ptr
    sig.params.push(AbiParam::new(I64));

    sig.returns.push(AbiParam::new(I64));

    let mut func = Function::with_name_signature(UserFuncName::default(), sig);

    let mut func_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut func, &mut func_ctx);

    let block = builder.create_block();
    builder.seal_block(block);

    builder.append_block_params_for_function_params(block);
    builder.switch_to_block(block);

    let res = translate_expr(e, &mut builder, &block);
    builder.ins().return_(&[res]);
    builder.finalize();

    let mut builder = settings::builder();
    // JIT is better with not optimization since the "long" part is compilation
    builder.set("opt_level", "speed").unwrap();
    let flags = settings::Flags::new(builder);

    let isa = match isa::lookup(Triple::host()) {
        Err(err) => panic!("Error looking up target: {}", err),
        Ok(isa_builder) => isa_builder.finish(flags).unwrap(),
    };

    let mut ctx = Context::for_function(func);
    let mut ctrl_plane = ControlPlane::default();
    let code = ctx.compile(&*isa, &mut ctrl_plane).unwrap();

    let mut buffer = memmap2::MmapOptions::new()
        .len(code.code_buffer().len())
        .map_anon()
        .unwrap();

    buffer.copy_from_slice(code.code_buffer());

    let buffer = buffer.make_exec().unwrap();

    let ptr: JitFunctionPtr = unsafe { std::mem::transmute(buffer.as_ptr()) };

    JitFunction { buffer, ptr }
}
