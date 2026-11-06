# References, Labels and Scopes (v0.1)

References are **links to nodes**, not path strings. After parsing, every reference points at one node (an object,
array or scalar field). Resolution is performed once, statically, for the whole document; forward references are
allowed. Any failure makes the document invalid; nothing is ever replaced by `null`.

## 1. Pointer syntax
```ebnf
pointer  = "!" key_seg tail                      (* @!field  : field of the current object *)
         | "." [ seg ] tail                      (* @.  @.!.23.id  @.$prev : relative to the current object *)
         | ".." global_name tail                 (* @..Users.$user.id : start at a global label *)
         | "$prev" tail | "$next" tail           (* contextual variables *)
         | "$" name tail                         (* user variable label *)
         | identifier tail ;                     (* label, else field of the root object *)
tail     = { "." seg } ;
seg      = identifier | string | index | "!" | "$prev" | "$next" | "$" name ;
index    = "0" | digit19 { digit } ;
```
No whitespace inside a pointer. Segments: identifier/quoted string = field, index = array element, `!` = **structural
parent**, `$prev`/`$next` = neighbouring array element, `$name` = *explicit descent to a label* (§4).

## 2. Definitions
* **current object** of a location: the innermost *object* strictly containing the reference (skipping arrays).
  For a label expression `<x=expr>` it is the object that declares the label.
* **array element of a location**: the nearest node (the location itself, then its ancestors) whose parent is an array.
* **structural parent** (`!`): the container node (object *or* array) that contains a node. **Array indexing**
  (`.23`) selects an element of an array. These are different operations: in `@.!.23.id`, `.` is the current
  object, `!` moves *up* to the array that contains it, `.23` indexes that array, `.id` selects a field.
* `@!field` is shorthand for `@.field`.
* While walking a path, a reference node met in the middle is followed transparently
  (`lessons.attendees[0].user.gender` works even though `user` is `@users.0`).
* The *result* of a pointer is the node it lands on; if that node is itself a reference it is followed until a
  non-reference node is reached (reference chains collapse). A chain/loop of pure references
  (`a: @b, b: @a`, `a: @a.b`) is `E_CYCLIC_REFERENCE`. Cycles through *structure* (an object holding a reference to
  its ancestor) are allowed.

## 3. Labels
`<name>` declares that name for the object it appears in. `<name=pointer>` (the `@` is optional) binds the name to
the pointer's target instead (evaluated as if written in that object).

| form | rule |
|---|---|
| `<user>` lowercase first letter | **local label**: visible in its declaring object and everything inside it (lexical scope); inner declarations shadow outer ones; two declarations in the *same* object are `E_DUPLICATE_LABEL` |
| `<Users>` uppercase first letter | **global label**: visible everywhere; must be unique in the document (`E_DUPLICATE_LABEL`) |
| `<$previous=$prev>` | **variable label** (sigil `$`), scoped like a local label |

Names are ASCII `[A-Za-z][A-Za-z0-9_]*` (a leading `_` is `E_INVALID_LABEL_NAME`); `$prev`/`$next` are reserved
(`E_RESERVED_LABEL`). A first pointer segment `name` is looked up as a label visible from the reference; if no
label is visible it is a field of the root object. If **both** exist it is `E_AMBIGUOUS_NAME` (rename one). A
name that is only declared out of scope is `E_LABEL_OUT_OF_SCOPE` and the message shows how to reach it.

### Decision: local labels are not visible from outside
Section 3.4 of the original brief shows `users: [ { <mylable> id: 23 } ], user_id: @mylable.id`, while §3.5 states
local labels cannot be accessed outside their scope. These conflict; JSONX follows §3.5 (strict lexical scope):
the 3.4 example is rejected with `E_LABEL_OUT_OF_SCOPE` (see `tests/vectors/text/errors/label_out_of_scope`).
Use `@users.0.id`, `@users.$mylable.id`, or make it global (`<Mylable>`).

## 4. Explicit descent `.$name` and `@..`
`X.$name` searches X and everything structurally inside it (not through references) for a label declared as
`name` or `$name`, and yields its target. Zero matches: `E_UNDEFINED_LABEL`; several: `E_AMBIGUOUS_LABEL`.
This is the "explicit exposure" of local labels. `@..Users` forces a lookup in the **global** namespace, so
`@..Users.$user.id` reaches the local label `user` declared somewhere inside global `Users`. The number of dots
after `@` is therefore fixed at two (global start); upward navigation is spelled `!`.

## 5. Contextual variables
`$next` / `$prev` are the following / preceding element of the *array element of the location* (§2). They are
only valid inside a reference: a bare `$prev` value is `E_SYNTAX`. The compiler checks them statically:
* first element → `E_INVALID_PREV`; last element → `E_INVALID_NEXT`
* no enclosing array element at all (`{a:1, b:{c:@$prev}}`) → `E_INVALID_PREV`/`E_INVALID_NEXT`
* `@.$prev` uses the *current object* itself (not the lexically nearest element): inside `meta: {...}` nested in an
  element it fails because `meta` is not an array element, while `@$prev` there refers to the enclosing element's neighbour.

### Decision: the brief's `<$previous=$prev>` example
In the brief the label sits in the **root** object, where no previous array element exists, so by the brief's
own rules it is invalid (`E_INVALID_PREV`, vector `context_label_prev_root`). It is valid inside an array element
(vector `labels_variable`).

## 6. Error positions
Pointer errors are reported at the `@` of the pointer (or the `<` of a label declaration for label
expressions); label duplicates at the second declaration. Labels are registered in structural pre-order: an object's
own labels first, then its members in document order.

## 7. In-memory semantics
A resolved reference holds the target node. In Python `to_python()` / JS `toJS()` objects and arrays are shared by
identity (`d["rel"] is d["users"][1]`); references to scalars are copied by value. Rust/Go expose the target id/pointer.
