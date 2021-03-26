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
}