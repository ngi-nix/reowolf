# Dealing with "Infinite Types" in a Value-Based Language

## The Problem

In many languages constructing `struct`s with members that, when expanded, end up having a field that refers to the struct itself by value is illegal. This makes sense because, firstly, one could never actually construct such a `struct`, if one would want to construct a literal then one would be typing an infinite amount of time. Secondly, because even if the struct could be constructed, one would have to take up an infinite amount of memory. As a practical example: a binary tree can conceptually grow to an infinite size, and such an infinite tree cannot be constructed.

And so, in low-level programming languages one would replace these with pointers (or something equivalent) in order to deal with both points. The first point is no longer true, because the pointer can be `null` (in the example: this would terminate a node in the tree, making it a leaf of the tree). And secondly the size is no longer infinite and is calculable because a pointer will always have a fixed size.

In Reowolf we have value-based semantics. To have reasonable cache coherency, quick `struct`/`union` member access, and prevent memory (de)allocation overhead we want to put as much values onto a stack-like construct. For this reason we would like to precalculate the alignment and offset of each type and its members.

Point one above still stands: if one wishes to construct a type that never terminates, then the compiler should throw an error. An example of which would be:

```pdl
// Simple truly infinite type
struct Infinite { 
    Infinite member,
}

// Infinite type of cycle length 2
struct Left { Right other }
struct Right { Left other }

// Infinite type of cycle length 3
struct A { B b }
struct B { C c }
struct C { A a }

// Etcetera

// But we can also do this using unions
union A { B(B), C(C) }
union B { A(A), C(C) }
union C { A(A), B(B) }
```

If one wishes to express a type whose value causes it to terminate somewhere, the only option in this language is to use `union`s to indicate the optionality of a certain member. For example:

```pdl
// One option, allows setting branches individually
union Option<T>{ Some(T), None }
struct U32TreeExample {
    u32 value, 
    Option<TreeExample> left,
    Option<TreeExample> right,
}

// Another option, must be a well-formed binary tree
union U32TreeLink {
    Leaf,
    Node(U32TreeLink, U32TreeLink)
}
struct U32TreeNode {
    u32 value,
    U32TreeLink link,
}

// Another kind of thing
union List<T> { 
    End,
    Entry(T, List<T>)
}
```

These are all valid types, and we should be able to express them, but naively calculating their size causes one to run into issues. 

## Onwards to an Algorithm

So far one can see that `struct`s and `union`s are the elements that might constitute an infinite type. All of the other types (user defined `enum`s and all of the builtins like `u32`, `string`, `s64`, etc.) do not have any members and have a fixed size. Whenever there is a union in the type, we might have a type that is not truly infinite, but may terminate somewhere.

Imagine a graph containing all user-defined `struct` and `union` types. The edges in this graph are directional, and indicate that one type is embedded in the other. It is important to construct this graph on a per-member basis. `struct` members may have one edge running towards it, coming from the member's type. `union`s can have multiple edges running towards it, one for each embedded type in the union member. Note that types may refer to themselves.

Having constructed this graph, one can visualise the potentially infinite types by loops in the graph. A struct is potentially infinite if one of its members is part of a loop. A union is potentially infinite if all of its members are part of a loop. A potentially infinite type is a truly infinite type if the relevant loops all contain potentially infinite types. Or, conversely: a potentially infinite type is not actually infinite if the relevant loops contain a union that is not potentially infinite.

More practically: a potentially infinite struct (implying that at least one of its members is part of a type loop) is not infinite if each of its "looped members" is part of a loop which contains a non-infinite union. Likewise, a potentially infinite union (implying that all of its members are part of a loop) is not actually infinite if, for one of its members that contains a set of loops (because a union can embed multiple types per variant), each of those loops contain a non-infinite union.

And finally, in human terms: a type is not actually infinite if, given enough time, a programmer can express a literal of that type. So one may type (using the examples above):

```pdl
auto thing = List::Entry(3, List::Entry(2, List::Entry(1, List::End)));
auto flong = U32TreeExample{
    value: 0,
    left: Option::Some(U32TreeExample{
        value: 1,
        left: Option::None
    },
    right: Option::None
}
```

But one simply cannot express:

```pdl
auto cannot_construct = A::B(B::C(C::A(A::C(C::B( /* onwards, to infinity )))));
```

Now we arrive at several conditions to end up at a concrete algorithm. Firsly we have that types whose literals are expressable should be constructable in the language. Secondly, for the performance reasons mentioned above we would like to specify the memory layout for each type. So that includes the size and alignment of the type itself, and the offset (with implicit alignment and size) of each of its members. Thirdly, because we're going to have to put pointers somewhere in the aforementioned type loops (necessarily, because we have variably sized values, we need to perform some kind of allocation somewhere, hence we need to have pointers somewhere), we need these pointers to be placed consistently throughout the code. This last point is a design decision: we could decide to have some types including pointers to members, and some types excluding pointers to members. This would then require special conversion functions implemented when two values of the same type, but with a different memory layout, are converted into one another. This on top of the fact that each of the types containing a pointer will require a constructor and a destructor. Finally, albeit this point looks ahead a bit at the implementation, we desire that subsequent compilations of the same files, but potentially in a different order, result in the same layout of each of the types.

We may observe that only unions may break up potentially infinite types. So for each loop that contains a non-infinite union we need at least one union that is implemented using some kind of pointer. There are multiple ways we can fix the byte size of the union, thereby breaking the type loop:

1. Make each instance of the union a pointer to the data in that union: both the tag and the potentially embedded values. This has the upside of being relatively simple to implement. Furthermore the union itself will just have a fixed size. The downside is that for a tag lookup we have to follow the pointer, generally causing cache misses in the case that we just want to check the tag.
   
2. Keep the tag in a union value, but dynamically allocate all of the contents. This way the size of a union value becomes a tag and a pointer. On 32-bit systems this is most likely fine, on 64-bit systems (which is the norm, if we're not talking about embedded computers) alignment will generally cause the union size to bloat to 16-bytes. This kind of implementation has two subcases to consider. 
   
    - Allocate as much memory as the maximum union variant. This way changing the union variant will not force a reallocation. Instead we can use up the allocated memory.
      
    - Allocate as much memory as the variant requires. This way we will not waste any memory, at the cost of having to reallocate when the variant is changed.
    
3. Keep the tag in the union value, with the same up- and downsides as the previous point. Then dynamically allocate the members of the variants (that is, if we have `union Foo { Bar(CyclicType, u64, u64), Qux }`, then we will just make `CyclicType` a pointer). The obvious downside is that changing variants might cause (de)allocations. And if we have multiple members that cause a loop then we need to allocate each of them. The upside is that when we're only accessing the non-pointer members that they're likely already in the cache.

There are many more tiny variants here that are not explicitly stated. I'll document my reasons for choosing the variant I think works best:

- The whole idea of breaking the type cycles is not to allocate an infinite amount of memory, so always allocating the union is naturally out of the question. Furthermore type loops will generally only occur in particular datastructures that are potentially infinite: trees, linked lists, hashmaps, etc.
  
- The contract when using a union is that the byte size of a union value is the size of the largest member plus the size of the tag (plus padding). So allocating the union just once, with enough size to fit all possible allocated variants seems like a good idea to me. I suspect that in most cases these unions will express something along the lines of: there is nothing, or there is nothing.

Finally, as a note: all of this type-loop checking has to be performed per monomorphization. A polymorphic type specification itself does not need to be checked for type loops.

Lastly, we have to consider the point of making sure that multiple compilations, where the AST is constructed in a different order, hence where the types are "discovered" in a different order, will result in the same types. As a rather simple solution, instead of just marking one union in the type loop as pointerlike, all of the unions in the type loops will be implemented using some kind of pointer scheme.

## Concluding on the Algorithm

As an initial conclusion:

- All non-infinite unions in type loops are implemented using dynamic allocation to break the type loops and to prevent potentially infinite types from being truly infinite.
- The tag of the union will be a non-allocated value associated with the union. Considering that most of the time these kinds of unions will represent an option-like type (there is one more entry in the linked list, there is one more node in the tree, etc.) I think it is nice to not have cache misses when checking whether a value is present, or whether it is not present.
- Considering that we cannot allocate all of the time (because then we will still end up with an infinite amount of allocated memory), but I do not currently want to implement very complicated logic in the compiler for handling memory allocations, I will initially implement the following scheme: all union variants that do not contain any types part of a type loop will be stored on the stack, contributing to the byte size of the union. All variants that do contain such types will be fully allocated on the heap in one allocation. The size of the allocated region will be the size of the largest variant that requires dynamic allocation.

## Rough Specification of the Algorithm

I might be missing some stuff here, that stuff will probably pop up while I'm implementing this:

- 
   Perform the basic pre-compilation for symbol discovery and definition discovery. This constructs the AST such that we can parse the AST to determine what fields are present on each `struct` and what variants are used in each `union`.

-
    For each type added to the type table (including monomorphs where we have all of the polymorphic variables fully specified), we will traverse all of the members and their types. For each type we will check if was already checked not to be cyclic. If it was already completely checked, then all types up until the most recently checked type cannot be cyclic (another member might still make them cyclic). If the type was not completely checked, but previously encountered in our search for loops, then all of types from the first encounter of that type are considered part of a type loop.

- 
    Note that this algorithm is recursive in the sense that for structs we need to check each member for type loops. For unions we need to check each embedded type in each variant. So when we encounter a type loop for a specific set of members, we walk back and mark each of the unions along the way as requiring allocation. If there were no such unions then we immediately return an error. We keep all of the unions that require allocation around in a list.
  
- 
   While we're doing the above, if we encounter a union who has a member that does not exist in a type loop then we mark it as non-infinite.
   
- 
   After all of the above is done, we need to check all of the unions we have encountered. Necessarily if we started with a particular type, and recursively checked all of the embedded types, then we will discover *all* type loops. If none of the unions is non-infinite, then we throw an error (which might be kind of hard, because what kind of error do we show in the case of the `union A; union B; union C;` example?). With the algorithm above, we now know that if there are type loops, that each of them contains at least one union. With the condition that there is at least one non-infinite union we now know that the type is expressable. The constructed list of unions only contains unions that were part of a type loop.

   With that knowledge, to guarantee consistent compilation, we will decide to make each union in each type loop, whether it is infinite or non-infinite, a pointerlike union.

-
   A pointerlike union's value will consist of a tag, plus reserved size (with the proper alignment) for the largest non-pointerlike union variant if it is larger than the size of a pointer on the particular machine. If it is smaller, than we just need a tag, and a pointer that is properly aligned. So the union value will have a fixed size that is not dependent on the types that were part of the type cycle. I'll call the variants that were not part of type cycles "value variants", and parts that *were* part of type cycles "pointer variants".

Then, during runtime, as an initial implementation (we could be smarter if we know the old value and know the new value, but that would take a lot of static analysis, we're not yet at that point in the compiler), we will check the tag value whether the old and new values are pointers. If the old and new variants are both pointer variants, or when they're both value variants, then we do not (re)allocate. If we go from a value variant to a pointer variant then we allocate. If we go from a pointer variant to a value variant then we deallocate.

## Some Examples of the Algorithm

I wrote these before I started writing all of the above, I'll keep it here for some concrete examples of the algorithm.

### Truly Infinite Example with Structs

```pdl
// Most simple case
struct SelfReferential{ SelfReferential field }

// Slightly more complicated case
struct DualA { DualB b }
struct DualB { DualA a }
```

In some sense we would be able to compute the sizes of these datastructures, we simply make their fields pointers. Let's assume 64 bit systems for all of these examples. For just the first case we would then have that `SelfReferential` has a size of 8 bytes. However, we always need to allocate its member `field`. So this would always take up an infinite amount of memory. We disallow this case because we would never be able to construct a literal of these kinds of datatypes.

### Truly Infinite Example with Unions

```pdl
struct S { UOne one, UTwo two }
union UOne { Struct(S), One(UOne), Two(UTwo) }
union UTwo { One(UOne), Two(UTwo) } 
```

Again, here we have a case where we can compute the sizes. We make the unions have pointerlike contents. So the unions will be 16 bytes (8 bytes for the tag and padding for pointer alignment, then an 8 byte pointer). Hence the struct will be 32 bytes. But once more we can never construct a literal and instantiating an element would always take up an infinite amount of memory.

### Slightly Ambiguous Example

With the following example we get one union which is non-infinite.

```pdl
struct SEnd { u32 some_value }
struct SMiddle {
   SEnd this_ends,
   UOne union_one,
   UTwo union_two,
}
union UOne { One(UOne), Two(UTwo), Struct(SMiddle) }
union UTwo { One(UOne), Two(UTwo), End(u32) }
```

If we draw the type graph, we get something like:

```pdl
+--------+        +---------+        +-------+        +-------+
| struct |        | struct  | <----- | union | <----- | union |
| SEnd   | -----> | SMiddle | -----> | UOne  | -----> | UTwo  |
+--------+        +---------+        +-------+        +-------+
                                      |     ^          |     ^
                                      |     |          |     |
                                      \-----/          \-----/
```

For this case we can actually always construct a literal of `SMiddle`. Viewing the literal as a tree (each node a type instance) then we can terminate the tree branches using `UTwo::End`. However, if we would just make `UTwo` pointerlike, we cannot compute the size of `UOne` (it contains a reference to itself, so would grow infinitely in size). This hints at that we should make `UOne` pointerlike as well.

### Slightly Ambiguous Example, Version 2

Not really ambiguous, but just to show that not all unions which are part of a connected type graph need to be turned into pointerlike unions.

```pdl
union Value {
    Unsigned(u64),
    Signed(s64),
    Undefined
}
struct Node {
    Value value,
    Children children
}

union Children {
    One(Node),
    Two(Node, Node),
    None
}
```

We get a cycle between `Node` and `Children`, so we would want to implement `Children` as being pointerlike. But we don't have to make `Value` pointerlike. In terms of the described algorithm: `Value` is not part of a type loop, so is not considered a candidate for being pointerlike.

### Slightly Ambigious Example, Version 3

Contains part of the type graph that is not part of a type loop. So the associated union should not be pointerlike.

```pdl
// Parts that are outside of the type loop
struct SOutside {
    UOutside next,
}
union UOutside {
    Next(SInside)
}

// Parts that are inside the type loop
struct SInside {
    UInside next,
}
union UInside {
    Next(SInside),
    NoNext,
}
```

Here `UInside` should become pointerlike, but `UOutside` should remain a normal value.

### Which Union Breaks the Cycle?

```pdl
struct S { UOne one }
union UOne { Two(UTwo), Nope }
union UTwo { Struct(S), Nope }
```

Here we see that we have a type loop to which both unions contribute. We can either lay out `UOne` as pointerlike, or `UTwo`. Both would allow us to calculate the size of the types and make the type expressable. However we need consistent compilation. For this reason and this reason only (because it is much more efficient to only lay out one of the unions as a pointer) we have to make both unions pointerlike. Perhaps in the future some other consistent metric can be applied.
