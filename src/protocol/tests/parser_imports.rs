/// parser_imports.rs
///
/// Simple import tests

use super::*;

#[test]
fn test_module_import() {
    Tester::new("single domain name")
        .with_source("
        #module external
        struct Foo { int field }
        ")
        .with_source("
        import external;
        int caller() {
            auto a = external::Foo{ field: 0 };
            return a.field;
        }
        ")
        .compile()
        .expect_ok();

    Tester::new("multi domain name")
        .with_source("
        #module external.domain
        struct Foo { int field }
        ")
        .with_source("
        import external.domain;
        int caller() {
            auto a = domain::Foo{ field: 0 };
            return a.field;
        }
        ")
        .compile()
        .expect_ok();

    Tester::new("aliased domain name")
        .with_source("
        #module external
        struct Foo { int field }
        ")
        .with_source("
        import external as aliased;
        int caller() {
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
        struct Foo { int field }
        ")
        .with_source("
        import external::Foo;
        int caller() {
            auto a = Foo{ field: 1 };
            auto b = Foo{ field: 2 };
            return a.field + b.field;
        }")
        .compile()
        .expect_ok();

    Tester::new("specific aliased symbol")
        .with_source("
        #module external
        struct Foo { int field }
        ")
        .with_source("
        import external::Foo as Bar;
        int caller() {
            return Bar{ field: 0 }.field;
        }
        ")
        .compile()
        .expect_ok();

    // TODO: Re-enable once std lib is properly implemented
    // Tester::new("import all")
    //     .with_source("
    //     #module external
    //     struct Foo { int field }
    //     ")
    //     .with_source("
    //     import external::*;
    //     int caller() { return Foo{field:0}.field; }
    //     ")
    //     .compile()
    //     .expect_ok();
}

#[test]
fn test_multi_symbol_import() {
    Tester::new("specific symbols")
        .with_source("
        #module external
        struct Foo { byte f }
        struct Bar { byte b }
        ")
        .with_source("
        import external::{Foo, Bar};
        byte caller() {
            return Foo{f:0}.f + Bar{b:1}.b;
        }
        ")
        .compile()
        .expect_ok();

    Tester::new("aliased symbols")
        .with_source("
        #module external
        struct Foo { byte in_foo }
        struct Bar { byte in_bar }
        ")
        .with_source("
        import external::{Foo as Bar, Bar as Foo};
        byte caller() {
            return Foo{in_bar:0}.in_bar + Bar{in_foo:0}.in_foo;    
        }")
        .compile()
        .expect_ok();

    // TODO: Re-enable once std lib is properly implemented
    // Tester::new("import all")
    //     .with_source("
    //     #module external
    //     struct Foo { byte f };
    //     struct Bar { byte b };
    //     ")
    //     .with_source("
    //     import external::*;
    //     byte caller() {
    //         auto f = Foo{f:0};
    //         auto b = Bar{b:0};
    //         return f.f + b.b;
    //     }
    //     ")
    //     .compile()
    //     .expect_ok();
}

#[test]
fn test_illegal_import_use() {
    Tester::new("unexpected polymorphic args")
        .with_source("
        #module external
        struct Foo { byte f }
        ")
        .with_source("
        import external;
        byte caller() {
            auto foo = external::Foo<int>{ f: 0 };
            return foo.f;
        }
        ")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "the type Foo is not polymorphic");
        });

    Tester::new("mismatched polymorphic args")
        .with_source("
        #module external
        struct Foo<T>{ T f }
        ")
        .with_source("
        import external;
        byte caller() {
            auto foo = external::Foo<byte, int>{ f: 0 };
            return foo.f;
        }")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "expected 1 polymorphic")
            .assert_msg_has(0, "2 were specified");
        });

    Tester::new("module as type")
        .with_source("
        #module external
        ")
        .with_source("
        import external;
        byte caller() {
            auto foo = external{ f: 0 };
            return 0;
        }
        ")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "resolved to a namespace");
        });

    Tester::new("more namespaces than needed, not polymorphic")
        .with_source("
        #module external
        struct Foo { byte f }
        ")
        .with_source("
        import external;
        byte caller() {
            auto foo = external::Foo::f{ f: 0 };
            return 0;
        }")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "not fully resolve this identifier")
            .assert_msg_has(0, "able to match 'external::Foo'");
        });

    Tester::new("import from another import")
        .with_source("
        #module mod1
        struct Foo { byte f }
        ")
        .with_source("
        #module mod2
        import mod1::Foo;
        struct Bar { Foo f }
        ")
        .with_source("
        import mod2;
        byte caller() {
            auto bar = mod2::Bar{ f: mod2::Foo{ f: 0 } };
            return var.f.f;
        }")
        .compile()
        .expect_err()
        .error(|e| { e
            .assert_msg_has(0, "Could not resolve this identifier")
            .assert_occurs_at(0, "mod2::Foo");
        });
}

// TODO: Test incorrect imports:
//  1. importing a module
//  2. import something a module imports
//  3. import something that doesn't exist in a module