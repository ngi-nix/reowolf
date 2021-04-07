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

#[test]
fn test_enum_monomorphs() {
    Tester::new_single_source_expect_ok(
        "no polymorph",
        "
        enum Answer{ Yes, No }
        int do_it() { auto a = Answer::Yes; return 0; }
        "
    ).for_enum("Answer", |e| { e
        .assert_num_monomorphs(0);
    });

    Tester::new_single_source_expect_ok(
        "single polymorph",
        "
        enum Answer<T> { Yes, No }
        int instantiator() {
            auto a = Answer<byte>::Yes;
            auto b = Answer<byte>::No;
            auto c = Answer<int>::Yes;
            auto d = Answer<Answer<Answer<long>>>::No;
            return 0;
        }
        "
    ).for_enum("Answer", |e| { e
        .assert_num_monomorphs(3)
        .assert_has_monomorph("byte")
        .assert_has_monomorph("int")
        .assert_has_monomorph("Answer<Answer<long>>");
    });
}

#[test]
fn test_union_monomorphs() {
    Tester::new_single_source_expect_ok(
        "no polymorph",
        "
        union Trinary { Undefined, Value(boolean) }
        int do_it() { auto a = Trinary::Value(true); return 0; }
        "
    ).for_union("Trinary", |e| { e
        .assert_num_monomorphs(0);
    });

    // TODO: Does this do what we want? Or do we expect the embedded monomorph
    //  Result<byte,int> to be instantiated as well? I don't think so.
    Tester::new_single_source_expect_ok(
        "polymorphs",
        "
        union Result<T, E>{ Ok(T), Err(E) }
        int instantiator() {
            auto a = Result<byte, boolean>::Ok(0);
            auto b = Result<boolean, byte>::Ok(true);
            auto c = Result<Result<byte, int>, Result<short, long>>::Err(Result::Ok(5));
            return 0;
        }
        "
    ).for_union("Result", |e| { e
        .assert_num_monomorphs(4)
        .assert_has_monomorph("byte;bool")
        .assert_has_monomorph("bool;byte")
        .assert_has_monomorph("Result<byte,int>;Result<short,long>")
        .assert_has_monomorph("short;long");
    });
}