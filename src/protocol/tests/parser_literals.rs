use super::*;

#[test]
fn test_binary_literals() {
    Tester::new_single_source_expect_ok("valid", "
        func test() -> u32 {
            u8  v1 = 0b0100_0010;
            u16 v2 = 0b10101010;
            u32 v3 = 0b10000001_01111110;
            u64 v4 = 0b1001_0110_1001_0110;

            return 0b10110;
        }
    ");

    Tester::new_single_source_expect_err("invalid character", "
        func test() -> u32 {
            return 0b10011001_10012001;
        }
    ").error(|e| { e.assert_msg_has(0, "incorrectly formatted binary number"); });

    Tester::new_single_source_expect_err("no characters", "
        func test() -> u32 { return 0b; }
    ").error(|e| { e.assert_msg_has(0, "incorrectly formatted binary number"); });

    Tester::new_single_source_expect_err("only separators", "
        func test() -> u32 { return 0b____; }
    ").error(|e| { e.assert_msg_has(0, "incorrectly formatted binary number"); });
}

#[test]
fn test_string_literals() {
    Tester::new_single_source_expect_ok("valid", "
        func test() -> string {
            auto v1 = \"Hello, world!\";
            auto v2 = \"\\t\\r\\n\\\\\"; // why hello there, confusing thing
            auto v3 = \"\";
            return \"No way, dude!\";
        }
    ").for_function("test", |f| { f
        .for_variable("v1", |v| { v.assert_concrete_type("string"); })
        .for_variable("v2", |v| { v.assert_concrete_type("string"); })
        .for_variable("v3", |v| { v.assert_concrete_type("string"); });
    });

    Tester::new_single_source_expect_err("unterminated simple", "
        func test() -> string { return \"'; }
    ").error(|e| { e
        .assert_num(1)
        .assert_occurs_at(0, "\"")
        .assert_msg_has(0, "unterminated");
    });

    Tester::new_single_source_expect_err("unterminated with preceding escaped", "
        func test() -> string { return \"\\\"; }
    ").error(|e| { e
        .assert_num(1)
        .assert_occurs_at(0, "\"\\")
        .assert_msg_has(0, "unterminated");
    });

    Tester::new_single_source_expect_err("invalid escaped character", "
        func test() -> string { return \"\\y\"; }
    ").error(|e| { e.assert_msg_has(0, "unsupported escape character 'y'"); });
}