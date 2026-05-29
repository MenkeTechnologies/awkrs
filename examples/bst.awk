# Binary search tree: insert each input key, print three traversals.
# Encoding: rk[i]=key, lt[i]=left child id (0=nil), rt[i]=right child id.

function bst_insert(root, key,   p) {
  if (root == 0) {
    rk[++nodes] = key; lt[nodes] = 0; rt[nodes] = 0
    return nodes
  }
  p = root
  while (1) {
    if (key < rk[p]) {
      if (lt[p] == 0) {
        rk[++nodes] = key; lt[nodes] = 0; rt[nodes] = 0
        lt[p] = nodes
        return root
      } else { p = lt[p] }
    } else {
      if (rt[p] == 0) {
        rk[++nodes] = key; lt[nodes] = 0; rt[nodes] = 0
        rt[p] = nodes
        return root
      } else { p = rt[p] }
    }
  }
}

function inorder(p)   { if (p == 0) return; inorder(lt[p]);   printf " %d", rk[p]; inorder(rt[p]) }
function preorder(p)  { if (p == 0) return; printf " %d", rk[p]; preorder(lt[p]);  preorder(rt[p]) }
function postorder(p) { if (p == 0) return; postorder(lt[p]); postorder(rt[p]); printf " %d", rk[p] }

{ root = bst_insert(root, $1 + 0) }

END {
  printf "inorder:";   inorder(root);   print ""
  printf "preorder:";  preorder(root);  print ""
  printf "postorder:"; postorder(root); print ""
}
