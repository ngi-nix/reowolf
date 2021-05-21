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
    Tester::new_single_source_expect_ok("slicing", "
        struct Holder<T> {
            T[] left,
            T[] right,
        }

        func create_array<T>(T first_index, T last_index) -> T[] {
            auto result = {};
            while (first_index < last_index) {
                // Absolutely rediculous, but we don't have builtin array functions yet...
                result = result @ { first_index };
                first_index += 1;
            }
            return result;
        }

        func create_holder<T>(T left_first, T left_last, T right_first, T right_last) -> Holder<T> {
            return Holder{
                left: create_array(left_first, left_last),
                right: create_array(right_first, right_last)
            };
        }

        // Another silly thing, we first slice the full thing. Then subslice a single
        // element, then concatenate. We always return an array of two things.
        func slicing_magic<T>(Holder<T> holder, u32 left_base, u32 left_amount, u32 right_base, u32 right_amount) -> T[] {
            auto left = holder.left[left_base..left_base + left_amount];
            auto right = holder.right[right_base..right_base + right_amount];
            return left[0..1] @ right[0..1];
        }

        func foo() -> bool {
            // Preliminaries:
            // 1. construct a holder, where:
            //      - left array will be [0, 1, 2, ...]
            //      - right array will be [2, 3, 4, ...]
            // 2. Perform slicing magic, always returning an array [3, 3]
            // 3. Make sure result is 6

            // But ofcourse, because we want to be silly, we will check this for
            // any possible integer type.
            auto created_u08 = create_holder<u8> (0, 5, 2, 8);
            auto created_u16 = create_holder<u16>(0, 5, 2, 8);
            auto created_u32 = create_holder<u32>(0, 5, 2, 8);
            auto created_u64 = create_holder<u64>(0, 5, 2, 8);

            auto result_u08 = slicing_magic(created_u08, 3, 2, 1, 2);
            auto result_u16 = slicing_magic(created_u16, 3, 2, 1, 2);
            auto result_u32 = slicing_magic(created_u32, 3, 2, 1, 2);
            auto result_u64 = slicing_magic(created_u64, 3, 2, 1, 2);

            auto result_s08 = slicing_magic(create_holder<s8> (0, 5, 2, 8), 3, 2, 1, 2);
            auto result_s16 = slicing_magic(create_holder<s16>(0, 5, 2, 8), 3, 2, 1, 2);
            auto result_s32 = slicing_magic(create_holder<s32>(0, 5, 2, 8), 3, 2, 1, 2);
            auto result_s64 = slicing_magic(create_holder<s64>(0, 5, 2, 8), 3, 2, 1, 2);

            return
                result_u08[0] + result_u08[1] == 6 &&
                result_u16[0] + result_u16[1] == 6 &&
                result_u32[0] + result_u32[1] == 6 &&
                result_u64[0] + result_u64[1] == 6 &&
                result_s08[0] + result_s08[1] == 6 &&
                result_s16[0] + result_s16[1] == 6 &&
                result_s32[0] + result_s32[1] == 6 &&
                result_s64[0] + result_s64[1] == 6;
        }
    ").for_function("foo", |f| {
        f.call_ok(Some(Value::Bool(true)));
    }).for_struct("Holder", |s| { s
        .assert_num_monomorphs(8)
        .assert_has_monomorph("u8")
        .assert_has_monomorph("u16")
        .assert_has_monomorph("u32")
        .assert_has_monomorph("u64")
        .assert_has_monomorph("s8")
        .assert_has_monomorph("s16")
        .assert_has_monomorph("s32")
        .assert_has_monomorph("s64");
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
fn test_field_selection_polymorphism() {
    // Bit silly, but just to be sure
    Tester::new_single_source_expect_ok("struct field shuffles", "
struct VecXYZ<T> { T x, T y, T z }
struct VecYZX<T> { T y, T z, T x }
struct VecZXY<T> { T z, T x, T y }
func modify_x<T>(T input) -> T {
    input.x = 1337;
    return input;
}

func foo() -> bool {
    auto xyz = VecXYZ<u16>{ x: 1, y: 2, z: 3 };
    auto yzx = VecYZX<u32>{ y: 2, z: 3, x: 1 };
    auto zxy = VecZXY<u64>{ x: 1, y: 2, z: 3 };

    auto mod_xyz = modify_x(xyz);
    auto mod_yzx = modify_x(yzx);
    auto mod_zxy = modify_x(zxy);

    return
        xyz.x == 1 && xyz.y == 2 && xyz.z == 3 &&
        yzx.x == 1 && yzx.y == 2 && yzx.z == 3 &&
        zxy.x == 1 && zxy.y == 2 && zxy.z == 3 &&
        mod_xyz.x == 1337 && mod_xyz.y == 2 && mod_xyz.z == 3 &&
        mod_yzx.x == 1337 && mod_yzx.y == 2 && mod_yzx.z == 3 &&
        mod_zxy.x == 1337 && mod_zxy.y == 2 && mod_zxy.z == 3;
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