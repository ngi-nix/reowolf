use super::*;

#[test]
fn test_assignment_operators() {
    fn construct_source(value_type: &str, value_initial: &str, value_op: &str) -> String {
        return format!(
            "func foo() -> {} {{
                {} value = {};
                value {};
                return value;
            }}",
            value_type, value_type, value_initial, value_op
        );
    }

    fn perform_test(name: &str, source: String, expected_value: Value) {
        Tester::new_single_source_expect_ok(name, source)
            .for_function("foo", move |f| {
                f.call(Some(expected_value));
            });
    }

    perform_test(
        "set",
        construct_source("u32", "1", "= 5"),
        Value::UInt32(5)
    );

    perform_test(
        "multiplied",
        construct_source("u32", "2", "*= 4"),
        Value::UInt32(8)
    );

    perform_test(
        "divided",
        construct_source("u32", "8", "/= 4"),
        Value::UInt32(2)
    );

    perform_test(
        "remained",
        construct_source("u32", "8", "%= 3"),
        Value::UInt32(2)
    );

    perform_test(
        "added",
        construct_source("u32", "2", "+= 4"),
        Value::UInt32(6)
    );

    perform_test(
        "subtracted",
        construct_source("u32", "6", "-= 4"),
        Value::UInt32(2)
    );

    perform_test(
        "shifted left",
        construct_source("u32", "2", "<<= 2"),
        Value::UInt32(8)
    );

    perform_test(
        "shifted right",
        construct_source("u32", "8", ">>= 2"),
        Value::UInt32(2)
    );

    perform_test(
        "bitwise and",
        construct_source("u32", "15", "&= 35"),
        Value::UInt32(3)
    );

    perform_test(
        "bitwise xor",
        construct_source("u32", "3", "^= 7"),
        Value::UInt32(4)
    );

    perform_test(
        "bitwise or",
        construct_source("u32", "12", "|= 3"),
        Value::UInt32(15)
    );
}

#[test]
fn test_binary_integer_operators() {
    fn construct_source(value_type: &str, code: &str) -> String {
        format!("
        func foo() -> {} {{
            {}
        }}
        ", value_type, code)
    }

    fn perform_test(test_name: &str, value_type: &str, code: &str, expected_value: Value) {
        Tester::new_single_source_expect_ok(test_name, construct_source(value_type, code))
            .for_function("foo", move |f| {
                f.call(Some(expected_value));
            });
    }

    perform_test(
        "bitwise_or", "u16",
        "auto a = 3; return a | 4;", Value::UInt16(7)
    );
    perform_test(
        "bitwise_xor", "u16",
        "auto a = 3; return a ^ 7;", Value::UInt16(4)
    );
    perform_test(
        "bitwise and", "u16",
        "auto a = 0b110011; return a & 0b011110;", Value::UInt16(0b010010)
    );
    perform_test(
        "shift left", "u16",
        "auto a = 0x0F; return a << 4;", Value::UInt16(0xF0)
    );
    perform_test(
        "shift right", "u64",
        "auto a = 0xF0; return a >> 4;", Value::UInt64(0x0F)
    );
    perform_test(
        "add", "u32",
        "auto a = 5; return a + 5;", Value::UInt32(10)
    );
    perform_test(
        "subtract", "u32",
        "auto a = 3; return a - 3;", Value::UInt32(0)
    );
    perform_test(
        "multiply", "u8",
        "auto a = 2 * 2; return a * 2 * 2;", Value::UInt8(16)
    );
    perform_test(
        "divide", "u8",
        "auto a = 32 / 2; return a / 2 / 2;", Value::UInt8(4)
    );
    perform_test(
        "remainder", "u16",
        "auto a = 29; return a % 3;", Value::UInt16(2)
    );
}