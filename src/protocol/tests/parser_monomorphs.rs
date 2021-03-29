/// parser_monomorphs.rs
///
/// Simple tests to make sure that all of the appropriate monomorphs are 
/// instantiated

use super::*;

#[test]
fn test_struct_monomorphs() {
    Tester::new_single_source_expect_ok(
        "no polymorph",
        "struct Integer{ int field }"
    ).for_struct("Integer", |s| { s
        .assert_num_monomorphs(0);
    });

    Tester::new_single_source_expect_ok(
        "single polymorph",
        "
        struct Number<T>{ T number }
        int instantiator() {
            auto a = Number<byte>{ number: 0 };
            auto b = Number<byte>{ number: 1 };
            auto c = Number<int>{ number: 2 };
            auto d = Number<long>{ number: 3 };
            auto e = Number<Number<short>>{ number: Number{ number: 4 }};
            return 0;
        }
        "
    ).for_struct("Number", |s| { s
        .assert_has_monomorph("byte")
        .assert_has_monomorph("short")
        .assert_has_monomorph("int")
        .assert_has_monomorph("long")
        .assert_has_monomorph("Number<short>")
        .assert_num_monomorphs(5);
    }).for_function("instantiator", |f| { f
        .for_variable("a", |v| {v.assert_concrete_type("Number<byte>");} )
        .for_variable("e", |v| {v.assert_concrete_type("Number<Number<short>>");} );
    });
}