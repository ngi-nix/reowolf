# Protocol Description Language

## Introduction

## Grammar

Beginning with the basics from which we'll construct the grammar, various characters and special variations thereof:

```
SP = " " // space 
HTAB = 0x09 // horizontal tab
VCHAR = 0x21-0x7E // visible ASCII character
VCHAR-ESCLESS = 0x20-0x5B | 0x5D-0x7E // visible ASCII character without "\"
WSP = SP | HTAB // whitespace
ALPHA = 0x41-0x5A | 0x61-0x7A // characters (lower and upper case)
DIGIT = 0x30-0x39 // digit
NEWLINE = (0x15 0x0A) | 0x0A // carriage return and line feed, or just line feed

// Classic backslash escaping to produce particular ASCII charcters
ESCAPE_CHAR = "\"
ESCAPED_CHARS = 
    ESCAPE_CHAR ESCAPE_CHAR |
    ESCAPE_CHAR "t" |
    ESCAPE_CHAR "r" |
    ESCAPE_CHAR "n" |
    ESCAPE_CHAR "0" |
    ESCAPE_CHAR "'" |
    ESCAPE_CHAR """
```

Which are composed into the following components of an input file that do not directly contribute towards the AST:

```
// asterisk followed by any ASCII char, excluding "/", or just any ASCII char without "*"
block-comment-contents = "*" (0x00-0x2E | 0x30-0x7E) | (0x20-0x29 | 0x2B-0x7E)
block-comment = "/*" block-comment-contents* "*/"
line-comment = "//" (WSP | VCHAR)* NEWLINE
comment = block-comment | line-comment
cw = (comment | WSP | NEWLINE)*
cwb = (comment | WSP | newline)+
```

Where it should be noted that the `cw` rule allows for not encountering any of the indicated characters, while the `cwb` rule expects at least one instance.

The following operators are defined:

```
binary-operator = "||" | "&&" | 
                  "!=" | "==" | "<=" | ">=" | "<" | ">" |
                  "|" | "&" | "^" | "<<" | ">>" |
                  "+" | "-" | "*" | "/" | "%"
assign-operator = "=" |
                  "|=" | "&=" | "^=" | "<<=" | ">>=" |
                  "+=" | "-=" | "*=" | "/=" | "%="
unary-operator = "++" | "--" | "+" | "-" | "~" | "!"
```

**QUESTION**: Do we include the pre/postfix "++" and "--" operators? They were introduced in C to reduce the amount of required characters. But is still necessary?

And to define various constants in the language, we allow for the following:

```
// Various integer constants, binary, octal, decimal, or hexadecimal, with a 
// utility underscore to enhance humans reading the characters. Allowing use to
// write something like 100_000_256 or 0xDEAD_BEEF
int-bin-char = "0" | "1"
int-bin-constant = "0b" int-bin-char (int-bin-char | "_")* // 0b0100_1110
int-oct-char = "0"-"7"
int-oct-constant = "0o" int-oct-char (int-oct-char | "_")* // 0o777
int-dec-constant = DIGIT (DIGIT | "_")* //
int-hex-char = DIGIT | "a"-"f" | "A"-"F" 
int-hex-constant = "0x" int-hex-char (int-hex-char | "_")* // 0xFEFE_1337
int-constant = int-bin-constant | int-oct-constant | int-dec-constant | int-hex-constant

// Floating point numbers
// TODO: Maybe support exponential notation? Seems silly for a networking
//  language, but might be useful? 
float-constant = DIGIT* "." DIGIT+

// Character constants: a single character. Its element may be an escaped 
// character or a VCHAR (excluding "'" and "\")
char-element = ESCAPED_CHARS | (0x20-0x26 | 0x28-0x5B | 0x5D-0x7E) 
char-constant = "'" char-element "'"

// Same thing for strings, but these may contain 0 or more characters
str-element = ESCAPED_CHARS | (0x20-0x21 | 0x23-0x5B | 0x5D-0x7E)
str-constant = """ str-element* """
```

Note that the integer characters are forced, somewhat arbitrarily without hampering the programmer's expressiveness, to start with a valid digit. Only then may one introduce the `_` character. And non-rigorously speaking characters may not contain an unescaped `'`-character, and strings may not contain an unescaped `"`-character.

We now introduce the various identifiers that exist within the language, we make a distinction between "any identifier" and "any identifier except for the builtin ones". Because we h

```
identifier-any = ALPHA | (ALPHA | DIGIT | "_")*
keyword = 
    "composite" | "primitive" |
    type-primitive | "true" | "false" | "null" |
    "struct" | "enum" |
    "if" | "else" |
    "while" | "break" | "continue" | "return" |
    "synchronous" | "assert" |
    "goto" | "skip" | "new" | "let"
builtin = "put" | "get" | "fires" | "create" | "assert"
identifier = identifier-any WITHOUT (keyword | builtin)

// Identifier with any number of prefixed namespaces
ns-identifier = (identifier "::")* identifier
```

We then start introducing the type system. Learning from the "mistake" of C/C++ of having types like `byte` and `short` with unspecified and compiler-dependent byte-sizes (followed by everyone using `stdint.h`), we use the Rust/Zig-like `u8`, `i16`, etc. Currently we will limit the programmer to not produce integers which take up more than 64 bits. Furthermore, as one is writing network code, it would be quite neat to be able to put non-byte-aligned integers into a struct in order to directly access meaningful bits. Hence, with restrictions introduced later, we will allow for types like `i4` or `u1`. When actually retrieving them or performing computations with them we will use the next-largest byte-size to operate on them in "registers".

**Question**: Difference between u1 and bool? Do we allow assignments between them? What about i1 and bool?

As the language semantics are value-based, we are prevented from returning information from functions through its arguments. We may only return information through its (single) return value. If we consider the common case of having to parse a series of bytes into a meaningful struct, we cannot return both the struct and a value as a success indicator. For this reason, we introduce algebraic datatypes (or: tagged unions, or: enums) as well.

Lastly, since functions are currently without internal side-effects (since functions cannot perform communication with components, and there is no functionality to interact "with the outside world" from within a function), it does not make sense to introduce the "void" type, as found in C/C++ to indicate that a function doesn't return anything of importance. However, internally we will allow for a "void" type, this will allow treating builtins such as "assert" and "put" like functions while constructing and evaluating the AST.

```
// The digits 1-64, without any leading zeros allowed, to allow specifying the
// signed and unsigned integer types
number-1-64 = NZ-DIGIT | (0x31-0x35 DIGIT) | ("6" 0x30-0x34)
type-signed-int = "i" number-1-64 // i1 through i64
type-unsigned-int = "u" number-1-64 // u1 through u64

// Standard floats and bools
type-float = "f32" | "f64"
type-bool = "bool"

// Messages, may be removed later
type-msg = "msg"

// Indicators of port types
type-port = "in" | "out"

// Unions and tagged unions, so we allow:
// enum SpecialBool { True, False }
// enum SpecialBool{True,False,}
// enum Tagged{Boolean(bool),SignedInt(i64),UnsignedInt(u64),Nothing}
type-union-element = identifier cw (("(" cw type cw ")") | ("=" cw int-constant))? 
type-union-def = "enum" cwb identifier cw "{" cw type-union-element (cw "," cw type-union-element)* (cw ",")? cw "}"

// Structs, so we allow:
// struct { u8 type, u2 flag0, u6 reserved }
type-struct-element = type cwb identifier
type-struct-def = "struct" cwb identifier cw "{" cw type-struct-element (cw "," cw type-struct-element)* (cw ",")? cw "}"

type-primitive = type-signed-int |
    type-unsigned-int |
    type-float |
    type-bool |
    type-msg |
    type-port
    
// A type may be a user-defined type (e.g. "struct Bla"), a namespaced
// user type (e.g. "Module::Bla"), or a non-namespaced primitive type. We 
// currently have no way (yet) to access nested modules, so we don't need to 
// care about identifier nesting.
type = type-primitive | ns-identifier
```

With these types, we need to introduce some extra constant types. Ones that are used to construct struct instances and ones that are used to construct/assign enums. These are constructed as:

```
// Struct literals
struct-constant-element = identifier cw ":" cw expr
struct-constant = ns-identifier cw "{" cw struct-constant-element (cw "," struct-constant-element)* cw "}"

enum-constant = ns-identifier "::" identifier cw "(" cw expr cw ")" 
```

Finally, we declare methods and field accessors as:

```
method = builtin | ns-identifier

field = "length" | identifier
```

**Question**: This requires some discussion. We allow for a "length" field on messages, and allow the definition of arrays. But if we wish to perform computation in a simple fashion, we need to allow for variable-length arrays of custom types. This requires builtin methods like "push", "pop", etc. But I suppose there is a much nicer way... In any case, this reminds me of programming in Fortran, which I definitely don't want to impose on other people (that, or I will force 72-character line lengths on them as well)

When we parse a particular source file, we may expect the following "pragmas" to be sprinkled at the top of the source
file. They may exist at any position in the global scope of a source file.

```
// A domain identifier is a dot-separated sequence of identifiers. As these are
// only used to identify modules we allow any identifier to be used in them. 
// The exception is the last identifier, which we, due to namespacing rules,
// force to be a non-reserved identifier.
domain-identifier = (identifier-any ".")* identifier

pragma-version = "#version" cwb int-constant cw ";" // e.g. #version 500
pragma-module = "#module" cwb domain-identifier cw ";" // e.g. #module hello.there

// Import, e.g.
// #import module.submodule // access through submodule::function(), or submodule::Type
// #import module.submodule as Sub // access through Sub::function(), or Sub::type
// #import module.submodule::* // access through function(), or Type
// #import module.submodule::{function} // access through function()
// #import module.submodule::{function as func, type} // access through func() or type

pragma-import-alias = cwb "as" cwb identifier
pragma-import-all = "::*"
pragma-import-single-symbol = "::" identifier pragma-import-alias?
pragma-import-multi-symbol = "::{" ...
    cw identifier pragma-import-alias? ...
    (cw "," cw identifier pragma-import-alias?)* ...
    (cw ",")? cw "}"
pragma-import = "#import" cwb domain-identifier ...
    (pragma-import-alias | pragma-import-all | pragma-import-single-symbol | pragma-import-multi-symbol)? 

// Custom pragmas for people which may be using (sometime, somewhere) 
// metaprogramming with pragmas
pragma-custom = "#" identifier-any (cwb VCHAR (VCHAR | WS)*) cw ";"

// Finally, a pragma may be any of the ones above
pragma = pragma-version | pragma-module | pragma-import | pragma-custom
```

Note that, different from C-like languages, we do require semicolons to exist at the end of a pragma statement. The reason is to prevent future hacks using the "\" character to indicate an end-of-line-but-not-really-end-of-line statements.

Apart from these pragmas, we can have component definitions, type definitions and function definitions within the source file. The grammar for these may be formulated as:

```
// Annotated types and function/component arguments
type-annotation = type (cw [])?
var-declaration = type-annotation cwb identifier
params-list = "(" cw (var-declaration (cw "," cw var-declaration)*)? cw ")"

// Functions and components
function-def = type-annotation cwb identifier cw params-list cw block
composite-def = "composite" cwb identifier cw params-list cw block
primitive-def = "primitive" cwb identifier cw params-list cw block
component-def = composite-def | primitive-def

// Symbol definitions now become
symbol-def = type-union-def | type-struct-def | function-def | component-def 
```

Using these rules, we can now describe the grammar of a single file as:

```
file = cw (pragma | symbol-def)* cw
```

Of course, we currently cannot do anything useful with our grammar, hence we have to describe blocks to let the functions and component definitions do something. To do so, we proceed as:

```
// channel a->b;, or channel a -> b;
channel-decl = channel cwb identifier cw "->" cw identifier cw ";"
// int a = 5, b = 2 + 3;
memory-decl = var-declaration cw "=" cw expression (cw "," cw identifier cw "=" cw expression)* cw ";"

stmt = block |
    identifier cw ":" cw stmt | // label
    "if" cw pexpr cw stmt (cw "else" cwb stmt)? |
    "while" cw pexpr cw stmt |
    "break" (cwb identifier)? cw ";" |
    "continue" (cwb identifier)? cw ";" |
    "synchronous" stmt |
    "return" cwb identifier cw ";" |
    "goto" cwb identifier cw ";" |
    "skip" cw ";" |
    "new" cwb method-expr cw ";" |
    expr cw ";"
    
// TODO: Add all the other expressions
// TODO: Also: add struct construction and enum construction
method-params-list = "(" cw (expr (cw "," cw expr)* )? cw ")"
method-expr = method cw method-params-list

enum-destructure-expr = "let" cw ns-identifier "::" identifier cw "(" cw identifier cw ")" cw "=" expr
enum-test-expr = ns-identifier "::" identifier cw "==" cw expr

block = "{" (cw (channel-decl | memory-decl | stmt))* cw "}"
```

Note that we have a potential collision of various expressions/statements. The following cases are of importance:

1. An empty block is written as `{}`, while an empty array construction is also written as `{}`.
2. Both function calls as enum constants feature the same construction syntax. That is: `foo::bar(expression)` may refer to a function call to `bar` in the namespace `foo`, but may also be the construction of enum `foo`'s `bar` variant (containing a value `expression`). These may be disambiguated using the type system.
3. The enumeration destructuring expression may collide with the constant enumeration literal. These may be disambiguated by looking at the inner value. If the inner value is an identifier and not yet defined as a variable, then it is a destructuring expression. Otherwise it must be interpreted as a constant enumeration. The enumeration destructuring expression must then be completed by it being a child of an binary equality operator. If not, then it is invalid syntax.

Finally, for consistency, there are additional rules to the enumeration destructuring. As a preamble: the language should allow programmers to express any kind of trickery they want, as long as it is correct. But programmers should be prevented from expressing something that is by definition incorrect/illogical. So enumeration destructuring (e.g. `Enum::Variant(bla) == expression`) should return a value with a special type (e.g. `EnumDestructureBool`) that may only reside within the testing expressions of `if` and `while` statements. Furthermore, this special boolean type only supports the logical-and (`&&`) operator. This way we prevent invalid expressions such as `if (Enum::Variant1(foo) == expr || Enum::Variant2(bar) == expr) { ... }`, but we do allow potentially valid expressions like `if (Enum::Variant1(foo) == expr_foo && Enum::Variant2(bar) == expr_bar) { ... }`.

**Question**: In the documentation for V1.0 we find the `synchronous cw (params-list cw stmt | block)` rule. Why the `params-list`?

**TODO**: Release constructions on memory declarations: as long as we have a write to it before a read we should be fine. Can be done once we add semantic analysis in order to optimize putting and getting port values.
**TODO**: Implement type inference, should be simpler once I figure out how to write a typechecker.
**TODO**: Add constants assigned in the global scope.
**TODO**: Add a runtime expression evaluator (probably before constants in global scope) to simplify expressions and/or remove impossible branches.