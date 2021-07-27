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
        .assert_msg_has(0, "infinitely large type");
    });
}

#[test]
fn test_invalid_union_type_loops() {
    Tester::new_single_source_expect_err(
        "self-referential union",
        "union Foo { One(Foo) }"
    ).error(|e| { e
        .assert_occurs_at(0, "Foo {")
        .assert_msg_has(0, "infinitely large type");
    });

    Tester::new_single_source_expect_err(
        "two-cycle union",
        "
        union UOne { Two(UTwo) }
        union UTwo { One(UOne) }
        "
    ).error(|e| { e
        .assert_occurs_at(0, "UOne")
        .assert_msg_has(0, "infinitely large type");
    });
}

#[test]
fn test_valid_loops() {
    // Linked-list like thing
    Tester::new_single_source_expect_ok(
        "linked list",
        "
        struct Entry { u32 number, Link next }
        union Link { Next(Entry), None }
        "
    ).for_struct("Entry", |s| {
        s.assert_size_alignment("Entry", 24, 8); // [4 'number', 4 'pad', x 'union tag', 8-x 'padding', 8 pointer]
    }).for_union("Link", |u| {
        u.assert_size_alignment("Link", 16, 8, 24, 8);
    });

    // Tree-like thing, version 1
    Tester::new_single_source_expect_ok(
        "tree, optional children",
        "
        union Option<T> { Some(T), None }
        struct Node {
            u32 value,
            Option<Node> left,
            Option<Node> right,
        }
        "
    ).for_struct("Node", |s| {
        s.assert_size_alignment("Node", 40, 8);
    }).for_union("Option", |u| {
        u.assert_size_alignment("Option<Node>", 16, 8, 40, 8);
    });

    // Tree-like thing, version 2
    Tester::new_single_source_expect_ok(
        "tree, with left/right link",
        "
        union Option<T> { Some(T), None }
        struct Link { Node left, Node right }
        struct Node { u32 value, Option<Link> link }
        "
    ).for_struct("Node", |s| {
        s.assert_size_alignment("Node", 24, 8);
    }).for_struct("Link", |s| {
        s.assert_size_alignment("Link", 48, 8);
    }).for_union("Option", |u| {
        u.assert_size_alignment("Option<Link>", 16, 8, 48, 8);
    });
}