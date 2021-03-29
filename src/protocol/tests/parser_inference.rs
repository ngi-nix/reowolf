/// parser_inference.rs
///
/// Simple tests for the type inferences

use super::*;

#[test]
fn test_integer_inference() {
    Tester::new_single_source_expect_ok(
        "by arguments",
        "
        int call(byte b, short s, int i, long l) {
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
            .assert_concrete_type("byte");
        })
        .for_variable("s2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("short");
        })
        .for_variable("i2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("int");
        })
        .for_variable("l2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("long");
        });
    });

    Tester::new_single_source_expect_ok(
        "by assignment",
        "
        int call() {
            byte b1 = 0; short s1 = 0; int i1 = 0; long l1 = 0;
            auto b2 = b1;
            auto s2 = s1;
            auto i2 = i1;
            auto l2 = l1;
            return 0;
        }"
    ).for_function("call", |f| { f
        .for_variable("b2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("byte");
        })
        .for_variable("s2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("short");
        })
        .for_variable("i2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("int");
        })
        .for_variable("l2", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("long");
        });
    });
}

#[test]
fn test_binary_expr_inference() {
    Tester::new_single_source_expect_ok(
        "compatible types",
        "int call() {
            byte b0 = 0;
            byte b1 = 1;
            short s0 = 0;
            short s1 = 1;
            int i0 = 0;
            int i1 = 1;
            long l0 = 0;
            long l1 = 1;
            auto b = b0 + b1;
            auto s = s0 + s1;
            auto i = i0 + i1;
            auto l = l0 + l1;
            return i;
        }"
    ).for_function("call", |f| { f
        .for_expression_by_source(
            "b0 + b1", "+", 
            |e| { e.assert_concrete_type("byte"); }
        )
        .for_expression_by_source(
            "s0 + s1", "+", 
            |e| { e.assert_concrete_type("short"); }
        )
        .for_expression_by_source(
            "i0 + i1", "+", 
            |e| { e.assert_concrete_type("int"); }
        )
        .for_expression_by_source(
            "l0 + l1", "+", 
            |e| { e.assert_concrete_type("long"); }
        );
    });

    Tester::new_single_source_expect_err(
        "incompatible types", 
        "int call() {
            byte b = 0;
            long l = 1;
            auto r = b + l;
            return 0;
        }"
    ).error(|e| { e
        .assert_ctx_has(0, "b + l")
        .assert_msg_has(0, "cannot apply")
        .assert_occurs_at(0, "+")
        .assert_msg_has(1, "has type 'byte'")
        .assert_msg_has(2, "has type 'long'");
    });
}



#[test]
fn test_struct_inference() {
    Tester::new_single_source_expect_ok(
        "by function calls",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        Pair<T1, T2> construct<T1, T2>(T1 first, T2 second) { 
            return Pair{ first: first, second: second };
        }
        int fix_t1<T2>(Pair<byte, T2> arg) { return 0; }
        int fix_t2<T1>(Pair<T1, int> arg) { return 0; }
        int test() {
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
            .assert_concrete_type("byte");
        })
        .for_variable("second", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("int");
        })
        .for_variable("pair", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Pair<byte,int>");
        });
    });

    Tester::new_single_source_expect_ok(
        "by field access",
        "
        struct Pair<T1, T2>{ T1 first, T2 second }
        Pair<T1, T2> construct<T1, T2>(T1 first, T2 second) {
            return Pair{ first: first, second: second };
        }
        int test() {
            auto first = 0;
            auto second = 1;
            auto pair = construct(first, second);
            byte assign_first = 0;
            long assign_second = 1;
            pair.first = assign_first;
            pair.second = assign_second;
            return 0;
        }
        "
    ).for_function("test", |f| { f
        .for_variable("first", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("byte");
        })
        .for_variable("second", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("long");
        })
        .for_variable("pair", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Pair<byte,long>");
        });
    });

    Tester::new_single_source_expect_ok(
        "by nested field access",
        "
        struct Node<T1, T2>{ T1 l, T2 r }
        Node<T1, T2> construct<T1, T2>(T1 l, T2 r) { return Node{ l: l, r: r }; }
        int fix_poly<T>(Node<T, T> a) { return 0; }
        int test() {
            byte assigned = 0;
            auto thing = construct(assigned, construct(0, 1));
            fix_poly(thing.r);
            thing.r.r = assigned;
            return 0;
        }
        ",
    ).for_function("test", |f| { f
        .for_variable("thing", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Node<byte,Node<byte,byte>>");
        });
    });
}

#[test]
fn test_failed_polymorph_inference() {
    Tester::new_single_source_expect_err(
        "function call inference mismatch",
        "
        int poly<T>(T a, T b) { return 0; }
        int call() {
            byte first_arg = 5;
            long second_arg = 2;
            return poly(first_arg, second_arg);
        }
        "
    ).error(|e| { e
        .assert_num(3)
        .assert_ctx_has(0, "poly(first_arg, second_arg)")
        .assert_occurs_at(0, "poly")
        .assert_msg_has(0, "Conflicting type for polymorphic variable 'T'")
        .assert_occurs_at(1, "second_arg")
        .assert_msg_has(1, "inferred it to 'long'")
        .assert_occurs_at(2, "first_arg")
        .assert_msg_has(2, "inferred it to 'byte'");
    });

    Tester::new_single_source_expect_err(
        "struct literal inference mismatch",
        "
        struct Pair<T>{ T first, T second }
        int call() {
            byte first_arg = 5;
            long second_arg = 2;
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
        .assert_msg_has(1, "inferred it to 'long'")
        .assert_occurs_at(2, "first_arg")
        .assert_msg_has(2, "inferred it to 'byte'");
    });

    Tester::new_single_source_expect_err(
        "field access inference mismatch",
        "
        struct Holder<Shazam>{ Shazam a }
        int call() {
            byte to_hold = 0;
            auto holder = Holder{ a: to_hold };
            return holder.a;
        }
        "
    ).error(|e| { e
        .assert_num(3)
        .assert_ctx_has(0, "holder.a")
        .assert_occurs_at(0, ".")
        .assert_msg_has(0, "Conflicting type for polymorphic variable 'Shazam'")
        .assert_msg_has(1, "inferred it to 'byte'")
        .assert_msg_has(2, "inferred it to 'int'");
    });

    // TODO: Needs better error messages anyway, but this failed before
    Tester::new_single_source_expect_err(
        "by nested field access",
        "
        struct Node<T1, T2>{ T1 l, T2 r }
        Node<T1, T2> construct<T1, T2>(T1 l, T2 r) { return Node{ l: l, r: r }; }
        int fix_poly<T>(Node<T, T> a) { return 0; }
        int test() {
            byte assigned = 0;
            long another = 1;
            auto thing = construct(assigned, construct(another, 1));
            fix_poly(thing.r);
            thing.r.r = assigned;
            return 0;
        }
        ",
    );
}