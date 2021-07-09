/// parser_types.rs
///
/// Tests for (infinite) type construction

use super::*;

#[test]
fn test_invalid_struct_type_loops() {
    Tester::new_single_source_expect_err(
        "self-referential struct",
        "struct Foo { Foo foo }"
    ).error(|e| { e
        .assert_occurs_at(0, "Foo {")
        .assert_msg_has(0, "cyclic type");
    });
}

#[test]
fn test_invalid_union_type_loops() {
    Tester::new_single_source_expect_err(
        "self-referential union",
        "union Foo { One(Foo) }"
    ).error(|e| { e
        .assert_occurs_at(0, "Foo {")
        .assert_msg_has(0, "cyclic type");
    });

    Tester::new_single_source_expect_err(
        "two-cycle union",
        "
        union UOne { Two(UTwo) }
        union UTwo { One(UOne) }
        "
    ).error(|e| { e
        .assert_occurs_at(0, "UOne")
        .assert_msg_has(0, "cyclic type");
    });
}