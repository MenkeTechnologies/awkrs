# gawk extension: `@var(args)` calls the function whose name is held in `var`.
# (gawk's `@` syntax only accepts simple identifiers as the callee — array
# elements and field references aren't allowed there.)
function double(x) { return x * 2 }
function add(a, b) { return a + b }
function greet(name) { return "hello, " name }

BEGIN {
    fn = "double"
    print @fn(7)

    fn = "add"
    print @fn(10, 32)

    fn = "greet"
    print @fn("awk")
}
