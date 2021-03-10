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

## Conclusions

- 
    Type definitions may contain polymorphic arguments. These are simple identifiers. The polymorphic arguments are embedded in definitions, they are erased after type inference on the type's use (e.g. function call or struct instantiation) has finished.

- 
    While instantiating polymorphic types (e.g. `T something = call_a_func(3, 5)` or the inferred variant `let something = call_a_func(3, 5)`) we actually need to resolve these types. This implies:
    
    - Memory declarations will have their type embedded within the memory declaration itself. If we do not know the type yet, or only partially know the type, then we need to mark the type as being inferred.
    - Expressions will have their type inferred from the constraints the expression imposes on its arguments, and the types of the arguments themselves. Within an expression tree type information may flow from the root towards the leaves or from the leaves towards the root.
    
    This implies that type information in the AST must be fully specified. If the type information is not yet fully known then it must be embedded in some kind of datastructure that is used for type inference.

-
    A function definition with polyargs seems to behave a bit differently: During the initial definition parsing we may want to make sure that each type in the definition either resolves to a concrete type (optionally with polymorphic arguments) or to one of the polyargs. This would also provide information to the type inference algorithm.

## Conclusions - Parsing

During parsing we want to embed all of the information the programmer has provided to the compiler in the AST. So polyargs go inside type definitions, types that are supposed to be inferred are embedded in the AST, types that depend on polyargs also go into the AST. So we may have:

- Partially concrete types with embedded inferred types (e.g. `let[] list_of_things` is an `Array<auto>` or `Result<auto> thing = Result::Ok(5)` or `let thing = Result::Ok(5)`).
- Direct references to polyarg types (e.g. `T list_of_things` or `Result<T> thing = ...` or `T[][] array_of_arrays_yes_sir` and `Tuple2<T, T> = construct_a_thing()`)
- Fully inferred types (e.g. `auto thing = 5` or `auto result = call_a_thing()`)
- Completely specified types.

## Conclusions - Type Inference

During type inference and typechecking we need to determine all concrete types. So we will arrive at a fully specified type: every polymorphic type will have its polyargs specified. Likewise every inferred type will be fully specified as well.

All of this hints at having to specify two classes of types: `ParserType` and a `Type`. The `ParserType` can be:

- A builtin type (with embedded `ParserType`s where appropriate)
- An inferred type
- A polyarg type
- A symbolic type (with embedded `ParsedType`s where appropriate)

While the final `Type` can be:

- A builtin type (with embedded types where appropriate)
- A known user-defined type (with embedded types where appropriate)

Note that we cannot typecheck polymorphic connectors and functions until we monomorphize them.