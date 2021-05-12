use super::*;

#[test]
fn test_concatenate_operator() {
    Tester::new_single_source_expect_ok(
        "concatenate and check values",
        "
        // Too see if we accept the polymorphic arg
        func check_pair<T>(T[] arr, u32 idx) -> bool {
            return arr[idx] == arr[idx + 1];
        }

        // Too see if we can check fields of polymorphs
        func check_values<T>(T[] arr, u32 idx, u32 x, u32 y) -> bool {
            auto value = arr[idx];
            return value.x == x && value.y == y;
        }

        struct Point2D {
            u32 x,
            u32 y,
        }

        // Could do this inline, but we're attempt to stress the system a bit
        func create_point(u32 x, u32 y) -> Point2D {
            return Point2D{ x: x, y: y };
        }

        // Again, more stressing: returning a heap-allocated thing
        func create_array() -> Point2D[] {
            return {
                create_point(1, 2),
                create_point(1, 2),
                create_point(3, 4),
                create_point(3, 4)
            };
        }

        // The silly checkamajig
        func foo() -> bool {
            auto lhs = create_array();
            auto rhs = create_array();
            auto total = lhs @ rhs;
            auto is_equal =
                check_pair(total, 0) &&
                check_pair(total, 2) &&
                check_pair(total, 4) &&
                check_pair(total, 6);
            auto is_not_equal =
                !check_pair(total, 0) ||
                !check_pair(total, 2) ||
                !check_pair(total, 4) ||
                !check_pair(total, 6);
            auto has_correct_fields =
                check_values(total, 3, 3, 4) &&
                check_values(total, 4, 1, 2);
            return is_equal && !is_not_equal && has_correct_fields;
        }
        "
    ).for_function("foo", |f| {
        f.call(Some(Value::Bool(true)));
    });
}

#[test]
fn test_struct_fields() {
    Tester::new_single_source_expect_ok("struct field access",
"
struct Nester<T> {
    T v,
}

func make<T>(T inner) -> Nester<T> {
    return Nester{ v: inner };
}

func modify<T>(Nester<T> outer, T inner) -> Nester<T> {
    outer.v = inner;
    return outer;
}

func foo() -> bool {
    // Single depth modification
    auto original1 = make<u32>(5);
    auto modified1 = modify(original1, 2);
    auto success1 = original1.v == 5 && modified1.v == 2;

    // Multiple levels of modification
    auto original2 = make(make(make(make(true))));
    auto modified2 = modify(original2.v, make(make(false))); // strip one Nester level
    auto success2 = original2.v.v.v.v == true && modified2.v.v.v == false;

    return success1 && success2;
}
").for_function("foo", |f| {
        f.call(Some(Value::Bool(true)));
    });
}