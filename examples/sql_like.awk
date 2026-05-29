# Tiny SQL-ish query engine over a CSV file.
# Query (first input line):
#   SELECT c1,c2,... [WHERE col OP value [AND col OP value ...]]
#                    [GROUP BY col [SUM col|COUNT|AVG col]]
#                    [ORDER BY col ASC|DESC]
#
# Data follows starting at line 2. First data line is the header (column names).
# OP in {==, !=, <, <=, >, >=, ~, !~}.
#
# This is a teaching-grade interpreter — no embedded commas/quotes in fields,
# the field separator is comma. Whitespace inside the query separates tokens.

function err(msg) { print "QUERY ERROR: " msg; exit 1 }

function tok_query(s,   i, c, t, n) {
  qn = 0
  n = length(s)
  i = 1
  while (i <= n) {
    c = substr(s, i, 1)
    if (c == " " || c == "\t") { i++; continue }
    if (c == ",") { qt[++qn] = ","; i++; continue }
    t = ""
    while (i <= n && substr(s, i, 1) != " " && substr(s, i, 1) != "\t" && substr(s, i, 1) != ",") {
      t = t substr(s, i, 1); i++
    }
    qt[++qn] = t
  }
}

function parse_select(   j) {
  if (toupper(qt[1]) != "SELECT") err("expected SELECT")
  j = 2; nsel = 0
  while (j <= qn) {
    u = toupper(qt[j])
    if (u == "WHERE" || u == "GROUP" || u == "ORDER") break
    if (qt[j] == ",") { j++; continue }
    sel[++nsel] = qt[j]; j++
  }
  return j
}

function parse_where(j,   u) {
  if (j > qn || toupper(qt[j]) != "WHERE") return j
  j++; nwh = 0
  while (j <= qn) {
    u = toupper(qt[j])
    if (u == "GROUP" || u == "ORDER") break
    if (u == "AND") { j++; continue }
    wh_col[++nwh] = qt[j]; wh_op[nwh] = qt[j+1]; wh_val[nwh] = qt[j+2]
    j += 3
  }
  return j
}

function parse_group(j,   u) {
  grp_col = ""; agg = ""; agg_col = ""
  if (j > qn || toupper(qt[j]) != "GROUP") return j
  j += 2  # skip GROUP BY
  grp_col = qt[j]; j++
  if (j <= qn) {
    u = toupper(qt[j])
    if (u == "SUM" || u == "AVG") { agg = u; agg_col = qt[j+1]; j += 2 }
    else if (u == "COUNT")        { agg = u;                    j++   }
  }
  return j
}

function parse_order(j) {
  ord_col = ""; ord_dir = "ASC"
  if (j > qn || toupper(qt[j]) != "ORDER") return j
  j += 2  # skip ORDER BY
  ord_col = qt[j]; j++
  if (j <= qn) ord_dir = toupper(qt[j])
  return j
}

function col_idx(name,   i) {
  for (i = 1; i <= hcols; i++) if (hdr[i] == name) return i
  return 0
}

function row_passes(   i, ci, v, op, rv, lhs) {
  for (i = 1; i <= nwh; i++) {
    ci = col_idx(wh_col[i]); if (ci == 0) err("unknown WHERE col " wh_col[i])
    lhs = row[ci]; op = wh_op[i]; rv = wh_val[i]
    if (op == "==") { if (lhs != rv && lhs + 0 != rv + 0) return 0 }
    else if (op == "!=") { if (lhs == rv) return 0 }
    else if (op == "<")  { if (!(lhs + 0 <  rv + 0)) return 0 }
    else if (op == "<=") { if (!(lhs + 0 <= rv + 0)) return 0 }
    else if (op == ">")  { if (!(lhs + 0 >  rv + 0)) return 0 }
    else if (op == ">=") { if (!(lhs + 0 >= rv + 0)) return 0 }
    else if (op == "~")  { if (!(lhs ~ rv)) return 0 }
    else if (op == "!~") { if   (lhs ~ rv)  return 0 }
    else err("unknown op " op)
  }
  return 1
}

BEGIN { FS = "," }

NR == 1 { tok_query($0); j = parse_select(); j = parse_where(j); j = parse_group(j); j = parse_order(j); next }
NR == 2 { hcols = NF; for (i = 1; i <= NF; i++) hdr[i] = $i; next }

{
  for (i = 1; i <= NF; i++) row[i] = $i
  if (!row_passes()) next

  if (grp_col != "") {
    gci = col_idx(grp_col); if (gci == 0) err("unknown GROUP col " grp_col)
    g = row[gci]
    grp_seen[g] = 1
    grp_count[g]++
    if (agg == "SUM" || agg == "AVG") {
      aci = col_idx(agg_col); if (aci == 0) err("unknown agg col " agg_col)
      grp_sum[g] += row[aci] + 0
    }
  } else {
    nr_out++
    for (i = 1; i <= nsel; i++) {
      ci = col_idx(sel[i]); if (ci == 0) err("unknown SELECT col " sel[i])
      out_row[nr_out, i] = row[ci]
    }
  }
}

END {
  # ----- grouped path: build one row per group key -----
  if (grp_col != "") {
    n = 0
    for (g in grp_seen) {
      n++
      grp_keys[n] = g
      if (agg == "COUNT")   grp_val[n] = grp_count[g]
      else if (agg == "SUM") grp_val[n] = grp_sum[g]
      else if (agg == "AVG") grp_val[n] = grp_sum[g] / grp_count[g]
      else                   grp_val[n] = ""
    }
    # ordering
    if (ord_col == grp_col || ord_col == "") {
      # sort group keys lex
      asort(grp_keys, sorted_keys)
      # asort destroys index association — re-pair via lookup
      for (i = 1; i <= n; i++) {
        g = sorted_keys[i]
        if      (agg == "COUNT") { v = grp_count[g] }
        else if (agg == "SUM")   { v = grp_sum[g] }
        else if (agg == "AVG")   { v = grp_sum[g] / grp_count[g] }
        else                     { v = "" }
        if (ord_dir == "DESC") { final_key[n - i + 1] = g; final_val[n - i + 1] = v } else { final_key[i] = g; final_val[i] = v }
      }
    } else {
      for (i = 1; i <= n; i++) { final_key[i] = grp_keys[i]; final_val[i] = grp_val[i] }
    }
    # header
    if (agg != "") printf "%s,%s\n", grp_col, agg
    else           printf "%s\n", grp_col
    for (i = 1; i <= n; i++) {
      if (agg == "AVG") printf "%s,%.2f\n", final_key[i], final_val[i]
      else if (agg == "")  printf "%s\n", final_key[i]
      else                 printf "%s,%s\n", final_key[i], final_val[i]
    }
    exit 0
  }

  # ----- ungrouped path: SELECT projection -----
  # header
  s = ""
  for (i = 1; i <= nsel; i++) { s = s (i==1 ? "" : ",") sel[i] }
  print s
  # rows; optional ORDER BY
  if (ord_col != "") {
    oci = col_idx(ord_col); if (oci == 0) err("unknown ORDER col " ord_col)
    # build a parallel sort-key array indexed by row#
    for (i = 1; i <= nr_out; i++) sort_idx[i] = i
    # simple insertion sort on sort_idx using row[oci] of the original full row
    # we already projected, so we need the full row again — fall back to using
    # one of the projected columns if it matches ord_col
    proj_idx = 0
    for (i = 1; i <= nsel; i++) if (sel[i] == ord_col) { proj_idx = i; break }
    if (proj_idx == 0) err("ORDER BY col not in SELECT list")
    for (i = 2; i <= nr_out; i++) {
      k = sort_idx[i]; vk = out_row[k, proj_idx]
      jj = i - 1
      while (jj >= 1) {
        jv = out_row[sort_idx[jj], proj_idx]
        if ((ord_dir == "DESC" && jv + 0 >= vk + 0) || (ord_dir != "DESC" && jv + 0 <= vk + 0)) break
        sort_idx[jj + 1] = sort_idx[jj]
        jj--
      }
      sort_idx[jj + 1] = k
    }
    for (i = 1; i <= nr_out; i++) {
      r = sort_idx[i]; s = ""
      for (j = 1; j <= nsel; j++) s = s (j==1 ? "" : ",") out_row[r, j]
      print s
    }
  } else {
    for (i = 1; i <= nr_out; i++) {
      s = ""
      for (j = 1; j <= nsel; j++) s = s (j==1 ? "" : ",") out_row[i, j]
      print s
    }
  }
}
