/// parser_inference.rs
///
/// Simple tests for the type inferences

use super::*;

#[test]
fn test_integer_inference() {
    Tester::new_single_source_expect_ok(
        "by arguments",
        "
        func call(u8 b, u16 s, u32 i, u64 l) -> u32 {
            auto b2 = b;
            auto s2 = s;
            auto i2 = i;
            auto l2 = l;
            return i2;
        }
        "
    ).for_function("call", |f| { f
        .for_variable("b2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("u8");
        })
        .for_variable("s2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("u16");
        })
        .for_variable("i2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("u32");
        })
        .for_variable("l2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("u64");
        });
    });

    Tester::new_single_source_expect_ok(
        "by assignment",
        "
        func call() -> u32 {
            u8 b1 = 0; u16 s1 = 0; u32 i1 = 0; u64 l1 = 0;
            auto b2 = b1;
            auto s2 = s1;
            auto i2 = i1;
            auto l2 = l1;
            return 0;
        }"
    ).for_function("call", |f| { f
        .for_variable("b2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("u8");
        })
        .for_variable("s2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("u16");
        })
        .for_variable("i2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("u32");
        })
        .for_variable("l2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("u64");
        });
    });
}

#[test]
fn test_binary_expr_inference() {
    Tester::new_single_source_expect_ok(
        "compatible types",
        "func call() -> s32 {
            s8 b0 = 0;
            s8 b1 = 1;
            s16 s0 = 0;
            s16 s1 = 1;
            s32 i0 = 0;
            s32 i1 = 1;
            s64 l0 = 0;
            s64 l1 = 1;
            auto b = b0 + b1;
            auto s = s0 + s1;
            auto i = i0 + i1;
            auto l = l0 + l1;
            return i;
        }"
    ).for_function("call", |f| { f
        .for_expression_by_source(
            "b0 + b1", "+", 
            |e| { e.assert_concrete_type("s8"); }
        )
        .for_expression_by_source(
            "s0 + s1", "+", 
            |e| { e.assert_concrete_type("s16"); }
        )
        .for_expression_by_source(
            "i0 + i1", "+", 
            |e| { e.assert_concrete_type("s32"); }
        )
        .for_expression_by_source(
            "l0 + l1", "+", 
            |e| { e.assert_concrete_type("s64"); }
        );
    });

    Tester::new_single_source_expect_err(
        "incompatible types", 
        "func call() -> s32 {
            s8 b = 0;
            s64 l = 1;
            auto r = b + l;
            return 0;
        }"
    ).error(|e| { e
        .assert_ctx_has(0, "b + l")
        .assert_msg_has(0, "cannot apply")
        .assert_occurs_at(0, "+")
        .assert_msg_has(1, "has type 's8'")
        .assert_msg_has(2, "has type 's64'");
    });
}



#[test]
fn test_struct_inference() {
    Tester::new_single_source_expect_ok(
        "by function calls",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        func construct<T1, T2>(T1 first, T2 second) -> Pair<T1, T2> {
            return Pair{ first: first, second: second };
        }
        func fix_t1<T2>(Pair<s8, T2> arg) -> s32 { return 0; }
        func fix_t2<T1>(Pair<T1, s32> arg) -> s32 { return 0; }
        func test() -> s32 {
            auto first = 0;
            auto second = 1;
            auto pair = construct(first, second);
            fix_t1(pair);
            fix_t2(pair);
            return 0;
        }
        "
    ).for_function("test", |f| { f
        .for_variable("first", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("s8");
        })
        .for_variable("second", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("s32");
        })
        .for_variable("pair", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Pair<s8,s32>");
        });
    });

    Tester::new_single_source_expect_ok(
        "by field access",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        func construct<T1, T2>(T1 first, T2 second) -> Pair<T1, T2> {
            return Pair{ first: first, second: second };
        }
        func test() -> s32 {
            auto first = 0;
            auto second = 1;
            auto pair = construct(first, second);
            s8 assign_first = 0;
            s64 assign_second = 1;
            pair.first = assign_first;
            pair.second = assign_second;
            return 0;
        }
        "
    ).for_function("test", |f| { f
        .for_variable("first", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("s8");
        })
        .for_variable("second", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("s64");
        })
        .for_variable("pair", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Pair<s8,s64>");
        });
    });

    Tester::new_single_source_expect_ok(
        "by nested field access",
        "
        struct Node<T1, T2>{ T1 l, T2 r }
        func construct<T1, T2>(T1 l, T2 r) -> Node<T1, T2> {
            return Node{ l: l, r: r };
        }
        func fix_poly<T>(Node<T, T> a) -> s32 { return 0; }
        func test() -> s32 {
            s8 assigned = 0;
            auto thing = construct(assigned, construct(0, 1));
            fix_poly(thing.r);
            thing.r.r = assigned;
            return 0;
        }
        ",
    ).for_function("test", |f| { f
        .for_variable("thing", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Node<s8,Node<s8,s8>>");
        });
    });
}

#[test]
fn test_enum_inference() {
    Tester::new_single_source_expect_ok(
        "no polymorphic vars",
        "
        enum Choice { A, B }
        func test_instances() -> s32 {
            auto foo = Choice::A;
            auto bar = Choice::B;
            return 0;
        }
        "
    ).for_function("test_instances", |f| { f
        .for_variable("foo", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Choice");
        })
        .for_variable("bar", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Choice");
        });
    });

    Tester::new_single_source_expect_ok(
        "one polymorphic var",
        "
        enum Choice<T>{
            A,
            B,
        }
        func fix_as_s8(Choice<s8> arg) -> s32 { return 0; }
        func fix_as_s32(Choice<s32> arg) -> s32 { return 0; }
        func test_instances() -> s32 {
            auto choice_s8 = Choice::A;
            auto choice_s32_1 = Choice::B;
            Choice<auto> choice_s32_2 = Choice::B;
            fix_as_s8(choice_s8);
            fix_as_s32(choice_s32_1);
            return fix_as_s32(choice_s32_2);
        }
        "
    ).for_function("test_instances", |f| { f
        .for_variable("choice_s8", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Choice<s8>");
        })
        .for_variable("choice_s32_1", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Choice<s32>");
        })
        .for_variable("choice_s32_2", |v| { v
            .assert_parser_type("Choice<auto>")
            .assert_concrete_type("Choice<s32>");
        });
    });

    Tester::new_single_source_expect_ok(
        "two polymorphic vars",
        "
        enum Choice<T1, T2>{ A, B, }
        func fix_t1<T>(Choice<s8, T> arg) -> s32 { return 0; }
        func fix_t2<T>(Choice<T, s32> arg) -> s32 { return 0; }
        func test_instances() -> s32 {
            Choice<s8, auto> choice1 = Choice::A;
            Choice<auto, s32> choice2 = Choice::A;
            Choice<auto, auto> choice3 = Choice::B;
            auto choice4 = Choice::B;
            fix_t1(choice1); fix_t1(choice2); fix_t1(choice3); fix_t1(choice4);
            fix_t2(choice1); fix_t2(choice2); fix_t2(choice3); fix_t2(choice4);
            return 0;
        }
        "
    ).for_function("test_instances", |f| { f
        .for_variable("choice1", |v| { v
            .assert_parser_type("Choice<s8,auto>")
            .assert_concrete_type("Choice<s8,s32>");
        })
        .for_variable("choice2", |v| { v
            .assert_parser_type("Choice<auto,s32>")
            .assert_concrete_type("Choice<s8,s32>");
        })
        .for_variable("choice3", |v| { v
            .assert_parser_type("Choice<auto,auto>")
            .assert_concrete_type("Choice<s8,s32>");
        })
        .for_variable("choice4", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Choice<s8,s32>");
        });
    });
}

#[test]
fn test_failed_variable_inference() {
    Tester::new_single_source_expect_err(
        "variable assignment mismatch",
        "
        func call() -> u32 {
            u16 val16 = 0;
            u32 val32 = 0;
            auto a = val16;
            a = val32;
            return 0;
        }
        "
    ).error(|e| { e
        .assert_num(2)
        .assert_msg_has(0, "type 'u16'")
        .assert_msg_has(1, "type 'u32'");
    });
}

#[test]
fn test_failed_polymorph_inference() {
    Tester::new_single_source_expect_err(
        "function call inference mismatch",
        "
        func poly<T>(T a, T b) -> s32 { return 0; }
        func call() -> s32 {
            s8 first_arg = 5;
            s64 second_arg = 2;
            return poly(first_arg, second_arg);
        }
        "
    ).error(|e| { e
        .assert_num(3)
        .assert_ctx_has(0, "poly(first_arg, second_arg)")
        .assert_occurs_at(0, "poly")
        .assert_msg_has(0, "Conflicting type for polymorphic variable 'T'")
        .assert_occurs_at(1, "second_arg")
        .assert_msg_has(1, "inferred it to 's64'")
        .assert_occurs_at(2, "first_arg")
        .assert_msg_has(2, "inferred it to 's8'");
    });

    Tester::new_single_source_expect_err(
        "struct literal inference mismatch",
        "
        struct Pair<T>{ T first, T second }
        func call() -> s32 {
            s8 first_arg = 5;
            s64 second_arg = 2;
            auto pair = Pair{ first: first_arg, second: second_arg };
            return 3;
        }
        "
    ).error(|e| { e
        .assert_num(3)
        .assert_ctx_has(0, "Pair{ first: first_arg, second: second_arg }")
        .assert_occurs_at(0, "Pair{")
        .assert_msg_has(0, "Conflicting type for polymorphic variable 'T'")
        .assert_occurs_at(1, "second_arg")
        .assert_msg_has(1, "inferred it to 's64'")
        .assert_occurs_at(2, "first_arg")
        .assert_msg_has(2, "inferred it to 's8'");
    });

    // Cannot really test literal inference error, but this comes close
    Tester::new_single_source_expect_err(
        "enum literal inference mismatch",
        "
        enum Uninteresting<T>{ Variant }
        func fix_t<T>(Uninteresting<T> arg) -> s32 { return 0; }
        func call() -> s32 {
            auto a = Uninteresting::Variant;
            fix_t<s8>(a);
            fix_t<s32>(a);
            return 4;
        }
        "
    ).error(|e| { e
        .assert_num(2)
        .assert_msg_has(0, "type 'Uninteresting<s8>'")
        .assert_msg_has(1, "type 'Uninteresting<s32>'");
    });

    Tester::new_single_source_expect_err(
        "field access inference mismatch",
        "
        struct Holder<Shazam>{ Shazam a }
        func call() -> s32 {
            s8 to_hold = 0;
            auto holder = Holder{ a: to_hold };
            return holder.a;
        }
        "
    ).error(|e| { e
        .assert_num(3)
        .assert_ctx_has(0, "holder.a")
        .assert_occurs_at(0, "holder.a")
        .assert_msg_has(0, "Conflicting type for polymorphic variable 'Shazam'")
        .assert_msg_has(1, "inferred it to 's8'")
        .assert_msg_has(2, "inferred it to 's32'");
    });

    // Silly regression test
    Tester::new_single_source_expect_err(
        "nested field access inference mismatch",
        "
        struct Node<T1, T2>{ T1 l, T2 r }
        func construct<T1, T2>(T1 l, T2 r) -> Node<T1, T2> { return Node{ l: l, r: r }; }
        func fix_poly<T>(Node<T, T> a) -> s32 { return 0; }
        func test() -> s32 {
            s8 assigned = 0;
            s64 another = 1;
            auto thing = construct(assigned, construct(another, 1));
            fix_poly(thing.r);
            thing.r.r = assigned;
            return 0;
        }
        ",
    );
}


#[test]
fn test_explicit_polymorph_argument() {
    // Failed because array was put at same type depth as u32. So interpreted
    // as a function with two polymorphic arguments
    Tester::new_single_source_expect_ok("explicit with array", "
    func foo<T>(T a, T b) -> T {
        return a @ b;
    }
    func test() -> u32 {
        return foo<u32[]>({1}, {2})[1];
    }").for_function("test", |f| { f
        .call_ok(Some(Value::UInt32(2)));
    });

    Tester::new_single_source_expect_err("multi-explicit with array", "
    func foo<A, B, C>(A a, B b) -> C {
        return (a @ b)[1];
    }
    func test() -> u32 {
        return foo<u32[], u32[], u32, u32>({1}, {2});
    }").error(|e| { e
        .assert_num(1)
        .assert_occurs_at(0, "foo<u32")
        .assert_msg_has(0, "expected 3")
        .assert_msg_has(0, "4 were provided");
    });

    // Failed because type inferencer did not construct polymorph errors by
    // considering that argument/return types failed against explicitly
    // specified polymorphic arguments
    Tester::new_single_source_expect_err("explicit polymorph mismatch", "
    func foo<T>(T a, T b) -> T { return a + b; }
    struct Bar<A, B>{A a, B b}
    func test() -> u32 {
        return foo<Bar<u32, u64>[]>(5, 6);
    }").error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "foo<Bar")
        .assert_msg_has(0, "'T' of 'foo'")
        .assert_occurs_at(1, "5, ")
        .assert_msg_has(1, "has type 'Bar<u32, u64>[]")
        .assert_msg_has(1, "inferred it to 'integerlike'");
    });

    // Similar to above, now for return type
    Tester::new_single_source_expect_err("explicit polymorph mismatch", "
    func foo<T>(T a, T b) -> T { return a + b; }
    func test() -> u32 {
        return foo<u64>(5, 6);
    }").error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "foo<u64")
        .assert_msg_has(0, "'T' of 'foo'")
        .assert_occurs_at(1, "foo<u64")
        .assert_msg_has(1, "has type 'u64'")
        .assert_msg_has(1, "return type inferred it to 'u32'");
    });
}