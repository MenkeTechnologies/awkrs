function inner(a) { a["x"] = "set_in_inner" }
function outer(b) { inner(b) }
BEGIN {
    outer(arr)
    print arr["x"]
}
