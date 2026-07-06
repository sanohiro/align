use inkwell::context::Context;
fn main() {
    let ctx = Context::create();
    let vty = ctx.bool_type().vec_type(4);
    let v = vty.const_zero();
    let b: inkwell::values::BasicValueEnum = v.into();
    println!("is_vector: {}, is_int: {}", b.is_vector_value(), b.is_int_value());
}
