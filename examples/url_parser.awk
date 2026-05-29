# URL parser — break input URLs into their components.
# Recognised form: scheme://[user[:pass]@]host[:port][/path][?query][#fragment]
# Input lines:  one URL per line.
# Output: lines of the form
#   url=<original>
#     scheme=<s>
#     user=<u>
#     pass=<p>
#     host=<h>
#     port=<p>
#     path=<path>
#     query=<q>
#     fragment=<f>
# Empty components are skipped.

function emit(url) {
  print "url=" url
  if (scheme != "")   print "  scheme=" scheme
  if (user != "")     print "  user=" user
  if (pass != "")     print "  pass=" pass
  if (host != "")     print "  host=" host
  if (port != "")     print "  port=" port
  if (path != "")     print "  path=" path
  if (query != "")    print "  query=" query
  if (fragment != "") print "  fragment=" fragment
}

NF == 0 { next }

{
  scheme = ""; user = ""; pass = ""; host = ""
  port = ""; path = ""; query = ""; fragment = ""

  url = $0
  rest = url

  # scheme
  if (match(rest, /^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//)) {
    scheme = substr(rest, 1, RLENGTH - 3)
    rest = substr(rest, RLENGTH + 1)
  }

  # fragment (trailing)
  if ((p = index(rest, "#")) > 0) {
    fragment = substr(rest, p + 1)
    rest = substr(rest, 1, p - 1)
  }

  # query
  if ((p = index(rest, "?")) > 0) {
    query = substr(rest, p + 1)
    rest = substr(rest, 1, p - 1)
  }

  # path
  if ((p = index(rest, "/")) > 0) {
    path = substr(rest, p)
    rest = substr(rest, 1, p - 1)
  }

  # authority = [user[:pass]@]host[:port]
  if ((p = index(rest, "@")) > 0) {
    auth = substr(rest, 1, p - 1)
    rest = substr(rest, p + 1)
    if ((c = index(auth, ":")) > 0) {
      user = substr(auth, 1, c - 1)
      pass = substr(auth, c + 1)
    } else {
      user = auth
    }
  }

  if ((p = index(rest, ":")) > 0) {
    host = substr(rest, 1, p - 1)
    port = substr(rest, p + 1)
  } else {
    host = rest
  }

  emit(url)
}
