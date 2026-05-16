function level3(a) { a["deep"] = "set" }
function level2(b) { level3(b) }
function level1(c) { level2(c) }
BEGIN {
    level1(arr)
    print arr["deep"]
}
