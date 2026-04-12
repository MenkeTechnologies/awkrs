BEGIN {
    # BEGINFILE/ENDFILE are tested in data-file mode, not BEGIN
    # This test checks basic PROCINFO keys
    print (PROCINFO["version"] != "" ? "ok" : "fail")
    print (PROCINFO["pid"] > 0 ? "ok" : "fail")
    print (PROCINFO["uid"] >= 0 ? "ok" : "fail")
    print (PROCINFO["gid"] >= 0 ? "ok" : "fail")
    print (PROCINFO["euid"] >= 0 ? "ok" : "fail")
    print (PROCINFO["egid"] >= 0 ? "ok" : "fail")
    print (PROCINFO["pgrpid"] >= 0 ? "ok" : "fail")
}
