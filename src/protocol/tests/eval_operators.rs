use super::*;

#[test]
fn test_assignment() {
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
    Tester::new_single_source_expect_ok(
        "set", construct_source("u32", "1", "= 5")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "multiplied", construct_source("u32", "2", "*= 4")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "divided", construct_source("u32", "8", "/= 4")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "remained", construct_source("u32", "8", "%= 3")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "added", construct_source("u32", "2", "+= 4")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "subtracted", construct_source("u32", "6", "-= 4")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "shifted left", construct_source("u32", "2", "<<= 2")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "shifted right", construct_source("u32", "8", ">>= 2")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "bitwise and", construct_source("u32", "3", "&= 2")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "bitwise xor", construct_source("u32", "3", "^= 7")
    ).for_function("foo", |f| { f.call(); });

    Tester::new_single_source_expect_ok(
        "bitwise or", construct_source("u32", "12", "|= 3")
    ).for_function("foo", |f| { f.call(); });
}

#[test]
fn test_function_call() {
    Tester::new_single_source_expect_ok("calling", "
    func add_two(u32 value) -> u32 {
        return value + 2;
    }
    func foo() -> u32 {
        return add_two(5);
    }
    ").for_function("foo", |f| { f.call(); });
}