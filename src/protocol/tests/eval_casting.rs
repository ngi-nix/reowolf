use super::*;

#[test]
fn test_valid_unsigned_casting() {
    Tester::new_single_source_expect_ok("cast u8", "
        func foo() -> bool {
            u64 large_width = 255;
            u8 small_width = 255;

            // Explicit casting
            auto large_exp_to_08 = cast<u8> (large_width);
            auto large_exp_to_16 = cast<u16>(large_width);
            auto large_exp_to_32 = cast<u32>(large_width);
            auto large_exp_to_64 = cast<u64>(large_width);

            auto small_exp_to_08 = cast<u8> (small_width);
            auto small_exp_to_16 = cast<u16>(small_width);
            auto small_exp_to_32 = cast<u32>(small_width);
            auto small_exp_to_64 = cast<u64>(small_width);

            // Implicit casting
            u8  large_imp_to_08 = cast(large_width);
            u16 large_imp_to_16 = cast(large_width);
            u32 large_imp_to_32 = cast(large_width);
            u64 large_imp_to_64 = cast(large_width);

            u8  small_imp_to_08 = cast(small_width);
            u16 small_imp_to_16 = cast(small_width);
            u32 small_imp_to_32 = cast(small_width);
            u64 small_imp_to_64 = cast(small_width);

            return
                large_exp_to_08 == 255 && large_exp_to_16 == 255 && large_exp_to_32 == 255 && large_exp_to_64 == 255 &&
                small_exp_to_08 == 255 && small_exp_to_16 == 255 && small_exp_to_32 == 255 && small_exp_to_64 == 255 &&
                large_imp_to_08 == 255 && large_imp_to_16 == 255 && large_imp_to_32 == 255 && large_imp_to_64 == 255 &&
                small_imp_to_08 == 255 && small_imp_to_16 == 255 && small_imp_to_32 == 255 && small_imp_to_64 == 255;
        }
    ").for_function("foo", |f| { f
        .call_ok(Some(Value::Bool(true)));
    });
}

#[test]
fn test_invalid_casting() {
    fn generate_source(input_type: &str, input_value: &str, output_type: &str) -> String {
        return format!("
        func foo() -> u32 {{
            {} value = {};
            {} result = cast(value);
            return 0;
        }}
        ", input_type, input_value, output_type);
    }

    fn perform_test(input_type: &str, input_value: &str, output_type: &str) {
        Tester::new_single_source_expect_ok(
            format!("invalid cast {} to {}", input_type, output_type),
            generate_source(input_type, input_value, output_type)
        ).for_function("foo", |f| {
            f.call_err(&format!("'{}' which doesn't fit in a type '{}'", input_value, output_type));
        });
    }

    // Not exhaustive, good enough
    let tests = [
        ("u16", "256", "u8"),
        ("u32", "256", "u8"),
        ("u64", "256", "u8"),
        ("u32", "65536", "u16"),
        ("u64", "65536", "u16"),
        ("s8", "-1", "u8"),
        ("s32", "-1", "u16"),
        ("s32", "65536", "u16"),
        ("s16", "-129", "s8"),
        ("s16", "128", "s8")
    ];

    for (input_type, input_value, output_type) in &tests {
        perform_test(input_type, input_value, output_type);
    }
}