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
            auto array_check = lhs == rhs && total == total;
            return is_equal && !is_not_equal && has_correct_fields && array_check;
        }
        "
    ).for_function("foo", |f| {
        f.call_ok(Some(Value::Bool(true)));
    });
}

#[test]
fn test_slicing_magic() {
    // TODO: Reimplement polymorphism, then retest with polymorphic types
    Tester::new_single_source_expect_ok("slicing", "
        struct Holder {
            u32[] left,
            u32[] right,
        }

        func create_array(u32 first_index, u32 last_index) -> u32[] {
            auto result = {};
            while (first_index < last_index) {
                // Absolutely rediculous, but we don't have builtin array functions yet...
                result = result @ { first_index };
                first_index += 1;
            }
            return result;
        }

        func create_holder(u32 left_first, u32 left_last, u32 right_first, u32 right_last) -> Holder {
            return Holder{
                left: create_array(left_first, left_last),
                right: create_array(right_first, right_last)
            };
        }

        // Another silly thing, we first slice the full thing. Then subslice a single
        // element, then concatenate. We always return an array of two things.
        func slicing_magic(Holder holder, u32 left_base, u32 left_amount, u32 right_base, u32 right_amount) -> u32[] {
            auto left = holder.left[left_base..left_base + left_amount];
            auto right = holder.right[right_base..right_base + right_amount];
            return left[0..1] @ right[0..1];
        }

        func foo() -> u32 {
            // left array will be [0, 1, 2, ...] and right array will be [2, 3, 4, ...]
            auto created = create_holder(0, 5, 2, 8);

            // in a convoluted fashion select the value 3 from the lhs and the value 3 from the rhs
            auto result = slicing_magic(create_holder(0, 5, 2, 8), 3, 2, 1, 2);

            // and return 3 + 3
            return result[0] + result[1];
        }
    ").for_function("foo", |f| {
        f.call_ok(Some(Value::UInt32(6)));
    });
}

#[test]
fn test_struct_fields() {
    Tester::new_single_source_expect_ok("struct field access", "
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
        f.call_ok(Some(Value::Bool(true)));
    });
}

#[test]
fn test_index_error() {
    Tester::new_single_source_expect_ok("indexing error", "
        func check_array(u32[] vals, u32 idx) -> u32 {
            return vals[idx];
        }

        func foo() -> u32 {
            auto array = {1, 2, 3, 4, 5, 6, 7};
            check_array(array, 7);
            return array[0];
        }
    ").for_function("foo", |f| {
        f.call_err("index 7 is out of bounds: array length is 7");
    });
}