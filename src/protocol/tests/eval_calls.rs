use super::*;

#[test]
fn test_function_call() {
    Tester::new_single_source_expect_ok("with literal arg", "
    func add_two(u32 value) -> u32 {
        return value + 2;
    }
    func foo() -> u32 {
        return add_two(5);
    }
    ").for_function("foo", |f| {
        f.call_ok(Some(Value::UInt32(7)));
    });

    Tester::new_single_source_expect_ok("with variable arg", "
    func add_two(u32 value) -> u32 {
        value += 1;
        return value + 1;
    }
    func foo() -> bool {
        auto initial = 5;
        auto result = add_two(initial);
        return initial == 5 && result == 7;
    }").for_function("foo", |f| {
        f.call_ok(Some(Value::Bool(true)));
    });
}