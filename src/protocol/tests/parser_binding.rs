use super::*;

#[test]
fn test_correct_binding() {
    Tester::new_single_source_expect_ok("binding bare", "
        enum TestEnum{ A, B }
        union TestUnion{ A(u32), B }
        struct TestStruct{ u32 field }

        func foo() -> u32 {
            auto lit_enum_a = TestEnum::A;
            auto lit_enum_b = TestEnum::B;
            auto lit_union_a = TestUnion::A(0);
            auto lit_union_b = TestUnion::B;
            auto lit_struct = TestStruct{ field: 0 };

            if (let test_enum_a = lit_enum_a)   { auto can_use = test_enum_a; }
            if (let test_enum_b = lit_enum_b)   { auto can_use = test_enum_b; }
            if (let test_union_a = lit_union_a) { auto can_use = test_union_a; }
            if (let test_union_b = lit_union_b) { auto can_use = test_union_b; }
            if (let test_struct = lit_struct)   { auto can_use = test_struct; }

            return 0;
        }
    ").for_function("foo", |f| { f
        .for_variable("test_enum_a", |v| { v.assert_concrete_type("TestEnum"); })
        .for_variable("test_enum_b", |v| { v.assert_concrete_type("TestEnum"); })
        .for_variable("test_union_a", |v| { v.assert_concrete_type("TestUnion"); })
        .for_variable("test_union_b", |v| { v.assert_concrete_type("TestUnion"); })
        .for_variable("test_struct", |v| { v.assert_concrete_type("TestStruct"); });
    });
}

#[test]
fn test_incorrect_binding() {
    Tester::new_single_source_expect_err("binding at statement level", "
        func foo() -> bool {
            return let a = 5;
        }
    ").error(|e| { e
        .assert_num(1)
        .assert_occurs_at(0, "let")
        .assert_msg_has(0, "only be used inside the testing expression");
    });

    Tester::new_single_source_expect_err("nested bindings", "
        struct Struct{ bool field }
        func foo() -> bool {
            if (let Struct{ field: let a = Struct{ field: false } } = Struct{ field: true }) {
                return test;
            }
            return false;
        }
    ").error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "let a = ")
        .assert_msg_has(0, "nested binding")
        .assert_occurs_at(1, "let Struct")
        .assert_msg_has(1, "outer binding");
    });
}

#[test]
fn test_boolean_ops_on_binding() {
    Tester::new_single_source_expect_ok("apply && to binding result", "
        union TestUnion{ Two(u16), Four(u32), Eight(u64) }
        func foo() -> u32 {
            auto lit_2 = TestUnion::Two(2);
            auto lit_4 = TestUnion::Four(4);
            auto lit_8 = TestUnion::Eight(8);

            // Testing combined forms of bindings
            if (
                let TestUnion::Two(test_2) = lit_2 &&
                let TestUnion::Four(test_4) = lit_4 &&
                let TestUnion::Eight(test_8) = lit_8
            ) {
                auto valid_2 = test_2;
                auto valid_4 = test_4;
                auto valid_8 = test_8;
            }

            // Testing in combination with regular expressions, and to the correct
            // literals
            if (let TestUnion::Two(inter_a) = lit_2 && 5 + 2 == 7)               { inter_a = 0; }
            if (5 + 2 == 7 && let TestUnion::Two(inter_b) = lit_2)               { inter_b = 0; }
            if (2 + 2 == 4 && let TestUnion::Two(inter_c) = lit_2 && 3 + 3 == 8) { inter_c = 0; }

            // Testing with the 'incorrect' target union
            if (let TestUnion::Four(nope) = lit_2 && let TestUnion::Two(zilch) = lit_8) { }

            return 0;
        }
    ").for_function("foo", |f| { f
        .for_variable("valid_2", |v| { v.assert_concrete_type("u16"); })
        .for_variable("valid_4", |v| { v.assert_concrete_type("u32"); })
        .for_variable("valid_8", |v| { v.assert_concrete_type("u64"); })
        .for_variable("inter_a", |v| { v.assert_concrete_type("u16"); })
        .for_variable("inter_b", |v| { v.assert_concrete_type("u16"); })
        .for_variable("inter_c", |v| { v.assert_concrete_type("u16"); });
    });

    Tester::new_single_source_expect_err("apply || before binding", "
        enum Test{ A, B }
        func foo() -> u32 {
            if (let a = Test::A || 5 + 2 == 7) {
                auto mission_impossible = 5;
            }
            return 0;
        }
    ").error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "let a")
        .assert_msg_has(0, "only the logical-and operator")
        .assert_occurs_at(1, "||")
        .assert_msg_has(1, "disallowed operation");
    });

    Tester::new_single_source_expect_err("apply || after binding", "
        enum Test{ A, B }
        func foo() -> u32 {
            if (5 + 2 == 7 || let b = Test::B) {
                auto magic_number = 7;
            }
            return 0;
        }
    ").error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "let b")
        .assert_msg_has(0, "only the logical-and operator")
        .assert_occurs_at(1, "||")
        .assert_msg_has(1, "disallowed operation");
    });

    Tester::new_single_source_expect_err("apply || before and after binding", "
        enum Test{ A, B }
        func foo() -> u32 {
            if (1 + 2 == 3 || (let a = Test::A && let b = Test::B) || (2 + 2 == 4)) {
                auto darth_vader_says = \"Noooooooooo\";
            }
            return 0;
        }
    ").error(|e| { e
        .assert_num(2)
        .assert_occurs_at(0, "let a")
        .assert_msg_has(0, "only the logical-and operator")
        .assert_occurs_at(1, "|| (let a")
        .assert_msg_has(1, "disallowed operation");
    });
}