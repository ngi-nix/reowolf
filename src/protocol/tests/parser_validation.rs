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
        struct Foo { s32 a }
        Foo bar(s32 arg) { return Foo{ a: arg }; }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields",
        "
        struct Foo { s32 a, s32 b }
        Foo bar(s32 arg) { return Foo{ a: arg, b: arg }; }
        "
    );

    Tester::new_single_source_expect_ok(
        "single field, explicit polymorph",
        "
        struct Foo<T>{ T field }
        Foo<s32> bar(s32 arg) { return Foo<s32>{ field: arg }; }
        "
    );

    Tester::new_single_source_expect_ok(
        "single field, implicit polymorph",
        "
        struct Foo<T>{ T field }
        s32 bar(s32 arg) {
            auto thingo = Foo{ field: arg };
            return arg;
        }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields, same explicit polymorph",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        s32 bar(s32 arg) {
            auto qux = Pair<s32, s32>{ first: arg, second: arg };
            return arg;
        }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields, same implicit polymorph", 
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        s32 bar(s32 arg) {
            auto wup = Pair{ first: arg, second: arg };
            return arg;
        }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields, different explicit polymorph",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        s32 bar(s32 arg1, s8 arg2) {
            auto shoo = Pair<s32, s8>{ first: arg1, second: arg2 };
            return arg1;
        }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple fields, different implicit polymorph",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        s32 bar(s32 arg1, s8 arg2) {
            auto shrubbery = Pair{ first: arg1, second: arg2 };
            return arg1;
        }
        "
    );
}

#[test]
fn test_incorrect_struct_instance() {
    Tester::new_single_source_expect_err(
        "reused field in definition",
        "struct Foo{ s32 a, s8 a }"
    ).error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "a }")
        .assert_msg_has(0, "defined more than once")
        .assert_occurs_at(1, "a, ")
        .assert_msg_has(1, "other struct field");
    });

    Tester::new_single_source_expect_err(
        "reused field in instance",
        "
        struct Foo{ int a, int b }
        int bar() {
            auto foo = Foo{ a: 5, a: 3 };
            return 0;
        }
        "
    ).error(|e| { e
        .assert_occurs_at(0, "a: 3")
        .assert_msg_has(0, "field is specified more than once");
    });

    Tester::new_single_source_expect_err(
        "missing field",
        "
        struct Foo { int a, int b }
        int bar() {
            auto foo = Foo{ a: 2 };
            return 0;
        }
        "
    ).error(|e| { e
        .assert_occurs_at(0, "Foo{")
        .assert_msg_has(0, "'b' is missing");
    });

    Tester::new_single_source_expect_err(
        "missing fields",
        "
        struct Foo { int a, int b, int c }
        int bar() {
            auto foo = Foo{ a: 2 };
            return 0;
        }
        "
    ).error(|e| { e
        .assert_occurs_at(0, "Foo{")
        .assert_msg_has(0, "[b, c] are missing");
    });
}

#[test]
fn test_correct_enum_instance() {
    Tester::new_single_source_expect_ok(
        "single variant",
        "
        enum Foo { A }
        Foo bar() { return Foo::A; }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple variants",
        "
        enum Foo { A=15, B = 0xF }
        Foo bar() { auto a = Foo::A; return Foo::B; }
        "
    );

    Tester::new_single_source_expect_ok(
        "explicit single polymorph",
        "
        enum Foo<T>{ A }
        Foo<int> bar() { return Foo::A; }
        "
    );

    Tester::new_single_source_expect_ok(
        "explicit multi-polymorph",
        "
        enum Foo<A, B>{ A, B }
        Foo<s8, int> bar() { return Foo::B; }
        "
    );
}

#[test]
fn test_incorrect_enum_instance() {
    Tester::new_single_source_expect_err(
        "variant name reuse",
        "
        enum Foo { A, A }
        Foo bar() { return Foo::A; }
        "
    ).error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "A }")
        .assert_msg_has(0, "defined more than once")
        .assert_occurs_at(1, "A, ")
        .assert_msg_has(1, "other enum variant is defined here");
    });

    Tester::new_single_source_expect_err(
        "undefined variant",
        "
        enum Foo { A }
        Foo bar() { return Foo::B; }
        "
    ).error(|e| { e
        .assert_num(1)
        .assert_msg_has(0, "variant 'B' does not exist on the enum 'Foo'");
    });
}

#[test]
fn test_correct_union_instance() {
    Tester::new_single_source_expect_ok(
        "single tag",
        "
        union Foo { A }
        Foo bar() { return Foo::A; }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple tags",
        "
        union Foo { A, B }
        Foo bar() { return Foo::B; }
        "
    );

    Tester::new_single_source_expect_ok(
        "single embedded",
        "
        union Foo { A(int) }
        Foo bar() { return Foo::A(5); }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple embedded",
        "
        union Foo { A(int), B(s8) }
        Foo bar() { return Foo::B(2); }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple values in embedded",
        "
        union Foo { A(int, s8) }
        Foo bar() { return Foo::A(0, 2); }
        "
    );

    Tester::new_single_source_expect_ok(
        "mixed tag/embedded",
        "
        union OptionInt { None, Some(int) }
        OptionInt bar() { return OptionInt::Some(3); } 
        "
    );

    Tester::new_single_source_expect_ok(
        "single polymorphic var",
        "
        union Option<T> { None, Some(T) }
        Option<int> bar() { return Option::Some(3); }"
    );

    Tester::new_single_source_expect_ok(
        "multiple polymorphic vars",
        "
        union Result<T, E> { Ok(T), Err(E), }
        Result<int, s8> bar() { return Result::Ok(3); }
        "
    );

    Tester::new_single_source_expect_ok(
        "multiple polymorphic in one variant",
        "
        union MaybePair<T1, T2>{ None, Some(T1, T2) }
        MaybePair<s8, int> bar() { return MaybePair::Some(1, 2); }
        "
    );
}

#[test]
fn test_incorrect_union_instance() {
    Tester::new_single_source_expect_err(
        "tag-variant name reuse",
        "
        union Foo{ A, A }
        "
    ).error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "A }")
        .assert_msg_has(0, "union variant is defined more than once")
        .assert_occurs_at(1, "A, ")
        .assert_msg_has(1, "other union variant");
    });

    Tester::new_single_source_expect_err(
        "embedded-variant name reuse",
        "
        union Foo{ A(int), A(s8) }
        "
    ).error(|e| { e 
        .assert_num(2)
        .assert_occurs_at(0, "A(s8)")
        .assert_msg_has(0, "union variant is defined more than once")
        .assert_occurs_at(1, "A(int)")
        .assert_msg_has(1, "other union variant");
    });

    Tester::new_single_source_expect_err(
        "undefined variant",
        "
        union Silly{ Thing(s8) }
        Silly bar() { return Silly::Undefined(5); }
        "
    ).error(|e| { e
        .assert_msg_has(0, "variant 'Undefined' does not exist on the union 'Silly'");
    });

    Tester::new_single_source_expect_err(
        "using tag instead of embedded",
        "
        union Foo{ A(int) }
        Foo bar() { return Foo::A; }
        "
    ).error(|e| { e
        .assert_msg_has(0, "variant 'A' of union 'Foo' expects 1 embedded values, but 0 were");
    });

    Tester::new_single_source_expect_err(
        "using embedded instead of tag",
        "
        union Foo{ A }
        Foo bar() { return Foo::A(3); }
        "
    ).error(|e| { e 
        .assert_msg_has(0, "The variant 'A' of union 'Foo' expects 0");
    });

    Tester::new_single_source_expect_err(
        "wrong embedded value",
        "
        union Foo{ A(int) }
        Foo bar() { return Foo::A(false); }
        "
    ).error(|e| { e
        .assert_occurs_at(0, "Foo::A")
        .assert_msg_has(0, "Failed to fully resolve")
        .assert_occurs_at(1, "false")
        .assert_msg_has(1, "has been resolved to 'int'")
        .assert_msg_has(1, "has been resolved to 'bool'");
    });
}