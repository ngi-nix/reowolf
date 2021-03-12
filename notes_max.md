# Noteworthy changes

- `if`, `while`, `synchronous`, `function`, `primitive` and `composite` body statements, if not yet a block, are all converted into a block for simpler parsing/compiling.

# Thinkings: Type System

## Basic Types

Builtin primitive types:

- Boolean: `bool`
- Unsigned integers: `u8`, `u16`, `u32`, `u64`
- Signed integers: `i8`, `i16`, `i32`, `i64`
- Decimal numbers: `f32`, `f64`
- Message-based: `in<T>`, `out<T>`, `msg`

Builtin compound types:

- Array: `array<T>`

User-specified types (excluding polymorphism for now):

- Connectors: `primitive`, `composite`
- Functions: `func`
- Enums: `enum`
- Unions: `enum`
- Structs: `struct`

## Polymorphic Type Definitions

Various types may be polymorphic. We will exclude builtin polymorphs from the following considerations (i.e. `in`, `out` and `array`, and their associated functions).

Connectors, functions, enums, unions and structs may all be polymorphic. We'll use the C++/rust/Java syntax for specifying polymorphic types:

```pdl
primitive fifo<T>(in<T> i, out<T> o) {
    // impl
}

T pick_one_of_two<T>(T one, T two) {
    // impl
}

enum Option<T>{ None, Some(T) }
enum Result<T, E>{ Ok(T), Err(E) }
struct Pair<T> { T first, T second }
```

This means that during the initial validation/linker phase all of the types may have polymorphic arguments. These polyargs have just an identifier, we can only determine the concrete type when we encounter it in another body where we either defer the type or where the user has explicitly instantiated the type.

For now we will use the C++-like trait/interfaceless polymorphism: once we know all of the polymorphic arguments we will try to monomorphize the type and check whether or not all calls make sense. This in itself is a recursive operation: inside the polymorph we may use other polymorphic types.

## Polymorphic Type Usage

Within functions and connectors we may employ polymorphic types. The specification of the polymorphic arguments is not required if they can be inferred. Polymorphic arguments may be partially specified by using somekind of throwaway `_` or `auto` type. When we are monomorphizing a type we must be able to fully determine all of the polymorphic arguments, if we can't then we throw a compiler error.

## Type Inference - Monomorphization

There are two parts to type inference: determining polymorphic variables and determining the `auto` variables. Among the polymorphic types we have: `struct`s, `enum`s, `union`s, `component`s and `function`s. The first three are datatypes and have no body to infer. The latter two are, for the lack of a better word, procedural types or proctypes.

We may only infer the types within a proctype if its polymorphic arguments are already inferred. Hence the type inference algorithm for a proctype does not work on the polymorphic types in its body. The type inference algorithm needs to work on all of the `auto` types in the body (potentially those of polymorphic types without explicitly specified polymorphic arguments).

If the type inference algorithm determines the arguments to a polymorphic type within the body of a proctype then we need to (recursively) instantiate a monomorph of that type. This process is potentially recursive in two ways:

1. **for datatypes**: when one instantiates a monomorph we suddenly know all of the embedded types within a datatype. At this point we may determine whether the type is potentially recursive or not. We may use this fact to mark struct members or union variants as pointerlike. We can at this point lay out the size and the offsets of the type.

2. **for proctypes**: when one instantiates a monomorph we can restart the inference process for the body of the function. During this process we may arrive at new monomorphs for datatypes or new monomorphs for function types.

## Type Inference - The Algorithm

So as determined above: when we perform type inference for a function we create a lookup for the assigned polymorphic variables and assign these where used in the expression trees. After this we end up with expression trees with `auto` types scattered here and there, potentially embedded within concrete types (e.g. `in<array<SomeStruct<byte, auto>>>`, an input port from which we get arrays of a monomorphized struct `SomeStruct` whose second argument should be inferred).

Since memory statements have already been converted into variable declarations (present in the appropriate scope) we only truly need to care about the expression trees that reside within a body. The one exception is the place where those expression trees are used: the expression tree used as the condition in an `if` statement must have a boolean return type. We have the following types of expressions, together with the constraints they impose on the inference algorithm (note that I will use the word "matching" types a lot. With that I will disregard implicit casting, for now. This is later fixed by inserting casting expressions that allow for (builtin) type conversion):

- **assignment**:
    Assignment expression (e.g. `a = 5 + 2` or `a <<= 2`). Where we make the distinction between:
    
    - **=**: We require that the RHS type matches the type of the LHS. If any side is known (partially) then the other side may be inferred. The return type is the type of the LHS.
    - **\*=**, **/=**, **%=**, **+=**, **-=**: We require that the LHS is of the appropriate number type and the RHS is of equal type. The return type is the type of the LHS.
    - **<<=**, **>>=**: The LHS is of an integer type and the RHS is of any integer type. The return type is the type of the LHS.
    - **&=**, **|=**, **^=**: The LHS and the RHS are of equal integer type. The return type is the type of the LHS.

- **conditional**:
    C-like ternary operator (e.g. `something() ? true : false`). We require that the test-expression is of boolean type. We require that the true-expression and the false-expression are of equal type. The return type is of the same type as the true/false-expression return type.

- **binary**:
    Binary operator, where we make the distinction between:
    - **@** (concatenate): *I might kick this one out*, requires that the LHS and RHS are arrays with equal subtype (`Array<T>`), or LHS and RHS are strings. The result is an array with the same subtype (`Array<T>`), or string.
    - **||**, **&&**: Both LHS and RHS must be booleans. The result is a boolean.
    - **|**, **&**, **^**: Both LHS and RHS must be of equal integer type. The return type is the type of the LHS.
    - **==**, **!=**: Both LHS and RHS must be of the same type. The return type is boolean
    - **<**, **>**, **<=**, **>=**: Both LHS and RHS must be of equal numeric type. The return type is boolean.
    - **<<**, **>>**: The LHS is of an integer type and the RHS if of any integer type. The return type is the type of the LHS.
    - **+**, **-**, **\***, **/**, **%**: The LHS and RHS are of equal integer type. The return type is the type of the LHS.

- **unary**:
    Unary operator, where we make the distinction between:
    **+**, **-**: Argument may be any numeric type, the return type is the same as the argument.
    **!**: Argument must be of boolean type, the return type is also boolean.
    **~**: Argument must be of any integer type, the return type is the same as the argument.
    **--**, **++**: Argument must be of any integer type. The return type is the same as the argument.

- **indexing expression**:
    Indexes into an array or a string (e.g. `variable[i]`). The index must always be a `usize`/`isize` integer type (with the appropriate implicit casts). The subject must always be of type `Array<T>` or string and the result is of type `T`. 

- **slicing expression**:
    Slices an array or a string (e.g. `some_array[5..calculate_the_end()]`). The beginning and ending indices must always be of **equal** `usize`/`isize` integer type. The subject must always be of type `Array<T>` or string, the return type is of the same type as the subject.

- **select expression**:
    Selects a member from a struct (e.g. `some_struct.some_field`). This expression can only be evaluated if we know the type of the subject (which must be a struct). If the field exists on that struct then we may determine the return type to be the type of that field.
    
- **array expression**:
    Constructs an array by explicitly specifying the members of that array. Lets assume for now that you cannot specify a string in this silly fashion. Then all of the array elements must have the same type `T`, and the result type of the expression is `Array<T>`.

- **call expression**: 
    Calls a particular function (e.g. `some_function<some_type>(some_arg)`). Non-polymorphic function arguments imply that the argument must be of the type of the argument. A non-polymorphic return type fixes the output type of the expression.

    In the polymorphic case the inference works the other way around: once we know the output type of the expression that is used as a polymorphic variable, then we can fix that particular polymorphic argument and feed that information to all places where that polymorphic argument is used.

    The same is true for the return type: if the return type of the call is polymorphic then the place where this return type is used determines the type of the polymorphic argument.

- **constant expression**: *This requires a lot of rework at some point.*
    A constant expression contains a literal of a specific type. Hence the output type of the literal is the type of the literal itself. In case of literal `struct`, `enum` and `union` instances the output type is that exact datatype.

    For the embedded types we have two cases: if the embedded type is not polymorphic and some expression is used at that position, then the embedded type must match the output type of the expression. If the embedded type is polymorphic then the output type of the expression determines the polymorphic argument. 

    In case of `string` literals the output type is also a `string`. However, in the case of integer literals we can only determine that the output type is some integer, the place where the literal is employed determines the integer type. It is valid to use this for byteshifting, but also for operating on floating-point types. In the case of decimal literals we can use these for operations on floating point types. But again: whether it is a `float` or a `double` depends on the place where the literal is used.

- **variable expression**:
    Refers to some variable declared somewhere. Hence the return type is the same as the type of the variable.