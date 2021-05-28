use super::*;

#[test]
fn test_binding_from_struct() {
    Tester::new_single_source_expect_ok("top level", "
        struct Foo{ u32 field }
        func foo() -> u32 {
            if (let Foo{ field: thing } = Foo{ field: 1337 }) { return thing; }
            return 0;
        }
    ").for_function("foo", |f| {
        f.call_ok(Some(Value::UInt32(1337)));
    });

    Tester::new_single_source_expect_ok("nested", "
        struct Nested<T>{ T inner }
        struct Foo{ u32 field }
        func make<T>(T val) -> Nested<T> {
            return Nested{ inner: val };
        }
        func foo() -> u32 {
            if (let Nested{ inner: Nested { inner: to_get } } = make(make(Foo{ field: 42 }))) {
                return to_get.field;
            }
            return 0;
        }
    ").for_function("foo", |f| {
        f.call_ok(Some(Value::UInt32(42)));
    });
}

#[test]
fn test_binding_from_array() {
    Tester::new_single_source_expect_ok("top level", "
        func foo() -> bool {
            // Mismatches in length
            bool failure1 = false;
            u32[] vals = { 1, 2, 3 };
            if (let {1, a} = vals) { failure1 = true; }

            bool failure2 = false;
            if (let {} = vals) { failure2 = true; }

            bool failure3 = false;
            if (let {1, 2, 3, 4} = vals) { failure3 = true; }

            // Mismatches in known values
            bool failure4 = false;
            if (let {1, 3, a} = vals) { failure4 = true; }

            // Matching with known variables
            auto constant_one = 1;
            auto constant_two = 2;
            auto constant_three = 3;
            bool success1 = false;
            if (
                let {binding_one, constant_two, constant_three} = vals &&
                let {constant_one, binding_two, constant_three} = vals &&
                let {constant_one, constant_two, binding_three} = vals
            ) {
                success1 = binding_one == 1 && binding_two == 2 && binding_three == 3;
            }

            // Matching with literals
            bool success2 = false;
            if (let {binding_one, 2, 3} = vals && let {1, binding_two, 3} = vals && let {1, 2, binding_three} = vals) {
                success2 = binding_one == 1 && binding_two == 2 && binding_three == 3;
            }

            // Matching all
            bool success3 = false;
            if (let {binding_one, binding_two, binding_three} = vals && let {1, 2, 3} = vals) {
                success3 = true;
            }

            return
                !failure1 && !failure2 && !failure3 && !failure4 &&
                success1 && success2 && success3;
        }
    ").for_function("foo", |f| { f
        .call_ok(Some(Value::Bool(true)));
    });
}

#[test]
fn test_binding_from_union() {
    Tester::new_single_source_expect_ok("option type", "
        union Option<T> { Some(T), None }

        func is_some<T>(Option<T> opt) -> bool {
            if (let Option::Some(throwaway) = opt) return true;
            else                                   return false;
        }
        func is_none<T>(Option<T> opt) -> bool {
            if (let Option::Some(throwaway) = opt) return false;
            else                                   return true;
        }

        func foo() -> u32 {
            // Hey look, we're so modern, we have algebraic discriminated sum datauniontypes
            auto something = Option::Some(5);
            auto nonething = Option<u32>::None;

            bool success1 = false;
            if (let Option::Some(value) = something && let Option::None = nonething) {
                success1 = value == 5;
            }

            bool success2 = false;
            if (is_some(something) && is_none(nonething)) {
                success2 = true;
            }

            bool success3 = true;
            if (let Option::None = something) success3 = false;
            if (let Option::Some(value) = nonething) success3 = false;

            bool success4 = true;
            if (is_none(something) || is_some(nonething)) success4 = false;

            if (success1 && success2 && success3 && success4) {
                if (let Option::Some(value) = something) return value;
            }

            return 0;
        }
    ").for_function("foo", |f| { f
        .call_ok(Some(Value::UInt32(5)));
    });
}

#[test]
fn test_binding_fizz_buzz() {
    Tester::new_single_source_expect_ok("am I employable?", "
        union Fizzable { Number(u32), FizzBuzz, Fizz, Buzz }

        func construct_fizz_buzz_very_slow(u32 num) -> Fizzable[] {
            u32 counter = 1;
            auto result = {};
            while (counter <= num) {
                auto value = Fizzable::Number(counter);
                if (counter % 5 == 0) {
                    if (counter % 3 == 0) {
                        value = Fizzable::FizzBuzz;
                    } else {
                        value = Fizzable::Buzz;
                    }
                } else if (counter % 3 == 0) {
                    value = Fizzable::Fizz;
                }

                result = result @ { value }; // woohoo, no array builtins!
                counter += 1;
            }

            return result;
        }

        func construct_fizz_buzz_slightly_less_slow(u32 num) -> Fizzable[] {
            u32 counter = 1;
            auto result = {};
            while (counter <= num) {
                auto value = Fizzable::Number(counter);
                if (counter % 5 == 0) {
                    if (counter % 3 == 0) {
                        value = Fizzable::FizzBuzz;
                    } else {
                        value = Fizzable::Buzz;
                    }
                } else if (counter % 3 == 0) {
                    value = Fizzable::Fizz;
                }

                result @= { value }; // woohoo, no array builtins!
                counter += 1;
            }

            return result;
        }

        func test_fizz_buzz(Fizzable[] fizzoid) -> bool {
            u32 idx = 0;
            bool valid = true;
            while (idx < length(fizzoid)) {
                auto number = idx + 1;
                auto is_div3 = number % 3 == 0;
                auto is_div5 = number % 5 == 0;

                if (let Fizzable::Number(got) = fizzoid[idx]) {
                    valid = valid && got == number && !is_div3 && !is_div5;
                } else if (let Fizzable::FizzBuzz = fizzoid[idx]) {
                    valid = valid && is_div3 && is_div5;
                } else if (let Fizzable::Fizz = fizzoid[idx]) {
                    valid = valid && is_div3 && !is_div5;
                } else if (let Fizzable::Buzz = fizzoid[idx]) {
                    valid = valid && !is_div3 && is_div5;
                } else {
                    // Impossibruuu!
                    valid = false;
                }

                idx += 1;
            }

            return valid;
        }

        func fizz_buzz() -> bool {
            // auto fizz_more_slow = construct_fizz_buzz_very_slow(100);
            // return test_fizz_buzz(fizz_more_slow);
            // auto fizz_less_slow = construct_fizz_buzz_slightly_less_slow(10000000);
            // return test_fizz_buzz(fizz_less_slow);

            auto fizz_more_slow2 = construct_fizz_buzz_very_slow(100);
            auto fizz_less_slow2 = construct_fizz_buzz_slightly_less_slow(100);
            return test_fizz_buzz(fizz_more_slow2) && test_fizz_buzz(fizz_less_slow2);
        }
    ").for_function("fizz_buzz", |f| { f
        .call_ok(Some(Value::Bool(true)));
    });
}