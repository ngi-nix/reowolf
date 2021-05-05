/// parser_monomorphs.rs
///
/// Simple tests to make sure that all of the appropriate monomorphs are 
/// instantiated

use super::*;

#[test]
fn test_struct_monomorphs() {
    Tester::new_single_source_expect_ok(
        "no polymorph",
        "struct Integer{ s32 field }"
    ).for_struct("Integer", |s| { s
        .assert_num_monomorphs(0);
    });

    Tester::new_single_source_expect_ok(
        "single polymorph",
        "
        struct Number<T>{ T number }
        func instantiator() -> s32 {
            auto a = Number<s8>{ number: 0 };
            auto b = Number<s8>{ number: 1 };
            auto c = Number<s32>{ number: 2 };
            auto d = Number<s64>{ number: 3 };
            auto e = Number<Number<s16>>{ number: Number{ number: 4 }};
            return 0;
        }
        "
    ).for_struct("Number", |s| { s
        .assert_has_monomorph("s8")
        .assert_has_monomorph("s16")
        .assert_has_monomorph("s32")
        .assert_has_monomorph("s64")
        .assert_has_monomorph("Number<s16>")
        .assert_num_monomorphs(5);
    }).for_function("instantiator", |f| { f
        .for_variable("a", |v| {v.assert_concrete_type("Number<s8>");} )
        .for_variable("e", |v| {v.assert_concrete_type("Number<Number<s16>>");} );
    });
}

#[test]
fn test_enum_monomorphs() {
    Tester::new_single_source_expect_ok(
        "no polymorph",
        "
        enum Answer{ Yes, No }
        func do_it() -> s32 { auto a = Answer::Yes; return 0; }
        "
    ).for_enum("Answer", |e| { e
        .assert_num_monomorphs(0);
    });

    Tester::new_single_source_expect_ok(
        "single polymorph",
        "
        enum Answer<T> { Yes, No }
        func instantiator() -> s32 {
            auto a = Answer<s8>::Yes;
            auto b = Answer<s8>::No;
            auto c = Answer<s32>::Yes;
            auto d = Answer<Answer<Answer<s64>>>::No;
            return 0;
        }
        "
    ).for_enum("Answer", |e| { e
        .assert_num_monomorphs(3)
        .assert_has_monomorph("s8")
        .assert_has_monomorph("s32")
        .assert_has_monomorph("Answer<Answer<s64>>");
    });
}

#[test]
fn test_union_monomorphs() {
    Tester::new_single_source_expect_ok(
        "no polymorph",
        "
        union Trinary { Undefined, Value(bool) }
        func do_it() -> s32 { auto a = Trinary::Value(true); return 0; }
        "
    ).for_union("Trinary", |e| { e
        .assert_num_monomorphs(0);
    });

    // TODO: Does this do what we want? Or do we expect the embedded monomorph
    //  Result<s8,s32> to be instantiated as well? I don't think so.
    Tester::new_single_source_expect_ok(
        "polymorphs",
        "
        union Result<T, E>{ Ok(T), Err(E) }
        func instantiator() -> s32 {
            s16 a_s16 = 5;
            auto a = Result<s8, bool>::Ok(0);
            auto b = Result<bool, s8>::Ok(true);
            auto c = Result<Result<s8, s32>, Result<s16, s64>>::Err(Result::Ok(5));
            auto d = Result<Result<s8, s32>, auto>::Err(Result<auto, s64>::Ok(a_s16));
            return 0;
        }
        "
    ).for_union("Result", |e| { e
        .assert_num_monomorphs(4)
        .assert_has_monomorph("s8;bool")
        .assert_has_monomorph("bool;s8")
        .assert_has_monomorph("Result<s8,s32>;Result<s16,s64>")
        .assert_has_monomorph("s16;s64");
    }).for_function("instantiator", |f| { f
        .for_variable("d", |v| { v
            .assert_parser_type("auto")
            .assert_concrete_type("Result<Result<s8,s32>,Result<s16,s64>>");
        });
    });
}