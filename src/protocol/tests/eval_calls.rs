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

#[test]
fn test_recursion() {
    // Single-chain
    Tester::new_single_source_expect_ok("factorial", "
    func horribly_slow_factorial(u32 term) -> u32 {
        if (term <= 0) { return 1; }

        return term * horribly_slow_factorial(term - 1);
    }
    func foo() -> u32 {
        return horribly_slow_factorial(10);
    }
    ").for_function("foo", |f| {
        f.call_ok(Some(Value::UInt32(3628800)));
    });

    // Multi-chain horribleness
    Tester::new_single_source_expect_ok("fibonacci", "
    func horribly_slow_fibo(u32 term) -> u32 {
        if (term <= 1) {
            return 1;
        }
        return horribly_slow_fibo(term - 2) + horribly_slow_fibo(term - 1);
    }
    func foo() -> u32 {
        return horribly_slow_fibo(10);
    }").for_function("foo", |f| {
        f.call_ok(Some(Value::UInt32(89)));
    });

    // Mutual recursion (in a contrived fashion, ofcourse)
    Tester::new_single_source_expect_ok("mutual recursion", "
    func collatz_even(u32 iter, u32 value) -> u32 {
        value = value / 2;
        if (value % 2 == 0) return collatz_even(iter + 1, value);
        else                return collatz_odd(iter + 1, value);
    }
    func collatz_odd(u32 iter, u32 value) -> u32 {
        if (value <= 1) return iter;

        value = 3 * value + 1;
        if (value % 2 == 0) return collatz_even(iter + 1, value);
        else                return collatz_odd(iter + 1, value);
    }
    func foo() -> u32 {
        return collatz_odd(1, 19);
    }
    ").for_function("foo", |f| {
        f.call_ok(Some(Value::UInt32(21)));
    });
}

#[test]
fn test_empty_blocks() {
    // Yes this is silly, but I managed to make this a bug before
    Tester::new_single_source_expect_ok("traversing empty statements", "
    func foo() -> u32 {
        auto val = 128;
        if (true) {}
        while (false) {}
        return val;
    }
    ").for_function("foo", |f| { f.call_ok(Some(Value::UInt32(128))); });
}