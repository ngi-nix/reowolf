/// parser_validation.rs
///
/// Simple tests for the validation phase
/// TODO: If semicolon behind struct definition: should be fine...

use super::*;

#[test]
fn test_correct_struct_instance() {
    Tester::new_single_source_expect_ok(
        "single field",
        "
        struct Foo { int a }
        Foo bar(int arg) { return Foo{ a: arg }; }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields",
        "
        struct Foo { int a, int b }
        Foo bar(int arg) { return Foo{ a: arg, b: arg }; }
        "
    );

    Tester::new_single_source_expect_ok(
        "single field, explicit polymorph",
        "
        struct Foo<T>{ T field }
        Foo<int> bar(int arg) { return Foo<int>{ field: arg }; }
        "
    );

    Tester::new_single_source_expect_ok(
        "single field, implicit polymorph",
        "
        struct Foo<T>{ T field }
        int bar(int arg) {
            auto thingo = Foo{ field: arg };
            return arg;
        }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields, same explicit polymorph",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        int bar(int arg) {
            auto qux = Pair<int, int>{ first: arg, second: arg };
            return arg;
        }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields, same implicit polymorph", 
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        int bar(int arg) {
            auto wup = Pair{ first: arg, second: arg };
            return arg;
        }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields, different explicit polymorph",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        int bar(int arg1, byte arg2) {
            auto shoo = Pair<int, byte>{ first: arg1, second: arg2 };
            return arg1;
        }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields, different implicit polymorph",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        int bar(int arg1, byte arg2) {
            auto shrubbery = Pair{ first: arg1, second: arg2 };
            return arg1;
        }
        "
    );
}