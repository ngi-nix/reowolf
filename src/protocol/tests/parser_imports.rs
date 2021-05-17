/// parser_imports.rs
///
/// Simple import tests

use super::*;

#[test]
fn test_module_import() {
    Tester::new("single domain name")
        .with_source("
        #module external
        struct Foo { s32 field }
        ")
        .with_source("
        import external;
        func caller() -> s32 {
            auto a = external::Foo{ field: 0 };
            return a.field;
        }
        ")
        .compile()
        .expect_ok();

    Tester::new("multi domain name")
        .with_source("
        #module external.domain
        struct Foo { s32 field }
        ")
        .with_source("
        import external.domain;
        func caller() -> s32 {
            auto a = domain::Foo{ field: 0 };
            return a.field;
        }
        ")
        .compile()
        .expect_ok();

    Tester::new("aliased domain name")
        .with_source("
        #module external
        struct Foo { s32 field }
        ")
        .with_source("
        import external as aliased;
        func caller() -> s32 {
            auto a = aliased::Foo{ field: 0 };
            return a.field;
        }
        ")
        .compile()
        .expect_ok();
}

#[test]
fn test_single_symbol_import() {
    Tester::new("specific symbol")
        .with_source("
        #module external
        struct Foo { s32 field }
        ")
        .with_source("
        import external::Foo;
        func caller() -> s32 {
            auto a = Foo{ field: 1 };
            auto b = Foo{ field: 2 };
            return a.field + b.field;
        }")
        .compile()
        .expect_ok();

    Tester::new("specific aliased symbol")
        .with_source("
        #module external
        struct Foo { s32 field }
        ")
        .with_source("
        import external::Foo as Bar;
        func caller() -> s32 {
            return Bar{ field: 0 }.field;
        }
        ")
        .compile()
        .expect_ok();

    Tester::new("import all")
        .with_source("
        #module external
        struct Foo { s32 field }
        ")
        .with_source("
        import external::*;
        func caller() -> s32 { return Foo{field:0}.field; }
        ")
        .compile()
        .expect_ok();
}

#[test]
fn test_multi_symbol_import() {
    Tester::new("specific symbols")
        .with_source("
        #module external
        struct Foo { s8 f }
        struct Bar { s8 b }
        ")
        .with_source("
        import external::{Foo, Bar};
        func caller() -> s8 {
            return Foo{f:0}.f + Bar{b:1}.b;
        }
        ")
        .compile()
        .expect_ok();

    Tester::new("aliased symbols")
        .with_source("
        #module external
        struct Foo { s8 in_foo }
        struct Bar { s8 in_bar }
        ")
        .with_source("
        import external::{Foo as Bar, Bar as Foo};
        func caller() -> s8 {
            return Foo{in_bar:0}.in_bar + Bar{in_foo:0}.in_foo;    
        }")
        .compile()
        .expect_ok();

    Tester::new("import all")
        .with_source("
        #module external
        struct Foo { s8 f }
        struct Bar { s8 b }
        ")
        .with_source("
        import external::*;
        func caller() -> s8 {
            auto f = Foo{f:0};
            auto b = Bar{b:0};
            return f.f + b.b;
        }
        ")
        .compile()
        .expect_ok();
}

#[test]
fn test_illegal_import_use() {
    Tester::new("unexpected polymorphic args")
        .with_source("
        #module external
        struct Foo { s8 f }
        ")
        .with_source("
        import external;
        func caller() -> s8 {
            auto foo = external::Foo<s32>{ f: 0 };
            return foo.f;
        }
        ")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "the type 'external::Foo' is not polymorphic");
        });

    Tester::new("mismatched polymorphic args")
        .with_source("
        #module external
        struct Foo<T>{ T f }
        ")
        .with_source("
        import external;
        func caller() -> s8 {
            auto foo = external::Foo<s8, s32>{ f: 0 };
            return foo.f;
        }")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "expected 1 polymorphic")
            .assert_msg_has(0, "2 were provided");
        });

    Tester::new("module as type")
        .with_source("
        #module external
        ")
        .with_source("
        import external;
        func caller() -> s8 {
            auto foo = external{ f: 0 };
            return 0;
        }
        ")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "expected a type but got a module");
        });

    Tester::new("missing type")
        .with_source("
        #module external
        struct Bar {}
        ")
        .with_source("
        import external::Foo;
        ")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "could not find symbol 'Foo' within module 'external'")
            .assert_occurs_at(0, "Foo");
        });

    Tester::new("import from another import")
        .with_source("
        #module mod1
        struct Foo { s8 f }
        ")
        .with_source("
        #module mod2
        import mod1::Foo;
        struct Bar { Foo f }
        ")
        .with_source("
        import mod2;
        func caller() -> s8 {
            auto bar = mod2::Bar{ f: mod2::Foo{ f: 0 } };
            return var.f.f;
        }")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "unknown type 'Foo' in module 'mod2'")
            .assert_msg_has(0, "module 'mod2' does import 'Foo'")
            .assert_occurs_at(0, "Foo");
        });
}