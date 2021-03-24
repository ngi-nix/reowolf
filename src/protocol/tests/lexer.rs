/// lexer.rs
///
/// Simple tests for the lexer. Only tests the lexing of the input source and
/// the resulting AST without relying on the validation/typing pass

use super::*;

#[test]
fn test_disallowed_inference() {
    Tester::new_single_source_expect_err(
        "argument auto inference",
            "int func(auto arg) { return 0; }"
    ).error(|e| { e
        .assert_msg_has(0, "inference is not allowed")
        .assert_occurs_at(0, "auto arg");
    });

    Tester::new_single_source_expect_err(
        "return type auto inference",
        "auto func(int arg) { return 0; }"
    ).error(|e| { e
        .assert_msg_has(0, "inference is not allowed")
        .assert_occurs_at(0, "auto func");
    });

    Tester::new_single_source_expect_err(
        "implicit polymorph argument auto inference",
        "int func(in port) { return port; }"
    ).error(|e| { e
        .assert_msg_has(0, "inference is not allowed")
        .assert_occurs_at(0, "in port");
    });

    Tester::new_single_source_expect_err(
        "explicit polymorph argument auto inference",
        "int func(in<auto> port) { return port; }"
    ).error(|e| { e
        .assert_msg_has(0, "inference is not allowed")
        .assert_occurs_at(0, "auto> port");
    });

    Tester::new_single_source_expect_err(
        "implicit polymorph return type auto inference",
        "in func(in<msg> a, in<msg> b) { return a; }"
    ).error(|e| { e
        .assert_msg_has(0, "inference is not allowed")
        .assert_occurs_at(0, "in func");
    });

    Tester::new_single_source_expect_err(
        "explicit polymorph return type auto inference",
        "in<auto> func(in<msg> a) { return a; }"
    ).error(|e| { e
        .assert_msg_has(0, "inference is not allowed")
        .assert_occurs_at(0, "auto> func");
    });
}

#[test]
fn test_simple_struct_definition() {
    Tester::new_single_source_expect_ok(
        "empty struct",
        "struct Foo{}"
    ).for_struct("Foo", |t| { t.assert_num_fields(0); });

    Tester::new_single_source_expect_ok(
        "single field, no comma",
        "struct Foo{ int field }"
    ).for_struct("Foo", |t| { t
        .assert_num_fields(1)
        .for_field("field", |f| {
            f.assert_parser_type("int");
        });
    });

    Tester::new_single_source_expect_ok(
        "single field, with comma",
        "struct Foo{ int field, }"
    ).for_struct("Foo", |t| { t
        .assert_num_fields(1)
        .for_field("field", |f| { f
            .assert_parser_type("int");
        });
    });

    Tester::new_single_source_expect_ok(
        "multiple fields, no comma",
        "struct Foo{ byte a, short b, int c }"
    ).for_struct("Foo", |t| { t
        .assert_num_fields(3)
        .for_field("a", |f| { f.assert_parser_type("byte"); })
        .for_field("b", |f| { f.assert_parser_type("short"); })
        .for_field("c", |f| { f.assert_parser_type("int"); });
    });

    Tester::new_single_source_expect_ok(
        "multiple fields, with comma",
        "struct Foo{
            byte a,
            short b,
            int c,
        }"
    ).for_struct("Foo", |t| { t
        .assert_num_fields(3)
        .for_field("a", |f| { f.assert_parser_type("byte"); })
        .for_field("b", |f| { f.assert_parser_type("short"); })
        .for_field("c", |f| { f.assert_parser_type("int"); });
    });
}