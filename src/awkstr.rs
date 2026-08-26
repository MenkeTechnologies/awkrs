//! [`AwkStr`] — the byte string an awk value holds.
//!
//! # Why bytes
//!
//! awk strings are byte strings. gawk, mawk and one-true-awk all pass an
//! arbitrary input byte through `$0`, a field, `substr`, `index` and `print`
//! untouched, and all three build one from `printf "%c", 233`. A Rust `String`
//! cannot hold a byte that is not part of valid UTF-8, so awkrs replaced every
//! such byte with `U+FFFD` on the way in — three bytes out where the references
//! emit one, and a value that can never match the byte it came from.
//!
//! # Why this representation
//!
//! - **`Vec<u8>` with a UTF-8 fast path** (this type). `regex::bytes::Regex` is
//!   already a dependency and already drives `RS` matching, so the byte-regex
//!   engine is proven here rather than new. The output half of the pipeline is
//!   already bytes — `print_buf`, `line_buf`, `ofs_bytes`, `ors_bytes` — so this
//!   meets it instead of converting at the boundary. [`AwkStr::as_utf8`] keeps
//!   the common case (valid UTF-8, usually ASCII) a borrow, not a copy.
//! - **`bstr::BString`** would be a new dependency for an API that is written
//!   here in a few hundred lines, and it still derefs to `[u8]` — every `&str`
//!   consumer in the tree breaks exactly the same way. It buys convenience, not
//!   a smaller change.
//! - **An enum (`Utf8(String) | Bytes(Vec<u8>)`)** was rejected on correctness,
//!   not effort: it gives one logical string two representations, so equality,
//!   hashing and array-subscript identity all have to normalise across the
//!   variants, and any site that forgets silently answers wrong — `a["x"]` and
//!   `a[Bytes("x")]` becoming separate entries is the same class of bug the
//!   `CONVFMT` subscript rules already had to be fixed for.

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;

/// The string payload of `crate::runtime::Value`: an arbitrary byte sequence.
///
/// Ordering and equality are over the bytes, which is what awk's string
/// comparison wants in a single-byte locale and what `strcmp` gives the
/// references. Valid UTF-8 sorts identically either way, so this only differs
/// from the old `String` ordering for input the old type could not hold.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AwkStr(Vec<u8>);

impl AwkStr {
    /// Empty string.
    #[inline]
    pub fn new() -> Self {
        AwkStr(Vec::new())
    }

    /// Empty string, usable in a `static` / `const`.
    ///
    /// `Vec::new` is const but `String::new().into()` is not, and the VM keeps a
    /// `static EMPTY_STR: Value` to hand out for a missing slot without
    /// allocating.
    #[inline]
    pub const fn new_const() -> Self {
        AwkStr(Vec::new())
    }

    /// Empty string with room for `n` bytes.
    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        AwkStr(Vec::with_capacity(n))
    }

    /// Take ownership of raw bytes — no validation, by design.
    #[inline]
    pub fn from_vec(v: Vec<u8>) -> Self {
        AwkStr(v)
    }

    /// The bytes. This is the value; everything else is a view of it.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The bytes, mutably — for in-place edits that keep the length rule.
    #[inline]
    pub fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }

    /// Give up the buffer.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Borrow as `&str` when the bytes are valid UTF-8 — the common case, and
    /// the one path that must not copy.
    ///
    /// `None` means the value holds a byte sequence no `&str` can name; the
    /// caller either works in bytes or falls back to [`Self::to_str_lossy`].
    #[inline]
    pub fn as_utf8(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// `&str` view, substituting `U+FFFD` for anything that is not valid UTF-8.
    ///
    /// Borrows when it can. Use this only where the answer is allowed to be
    /// approximate (diagnostics, `Display`); anything the program can observe
    /// should go through [`Self::as_bytes`] or [`Self::as_utf8`].
    #[inline]
    pub fn to_str_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    /// Owned lossy `String`. Same caveat as [`Self::to_str_lossy`].
    #[inline]
    pub fn to_lossy_string(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }

    /// Length in **bytes**, which is what `length()` reports in a single-byte
    /// locale in all three references.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `len() == 0`.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Truncate to empty, keeping the allocation.
    #[inline]
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Append a `&str`.
    #[inline]
    pub fn push_str(&mut self, s: &str) {
        self.0.extend_from_slice(s.as_bytes());
    }

    /// Append raw bytes.
    #[inline]
    pub fn push_bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }

    /// Append one byte — the operation `String` cannot offer.
    #[inline]
    pub fn push_byte(&mut self, b: u8) {
        self.0.push(b);
    }

    /// Append one `char`, UTF-8 encoded.
    #[inline]
    pub fn push_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.0.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }

    /// Append another `AwkStr`.
    #[inline]
    pub fn push_awkstr(&mut self, other: &AwkStr) {
        self.0.extend_from_slice(&other.0);
    }

    /// Byte-offset of the first occurrence of `needle`.
    #[inline]
    pub fn find_bytes(&self, needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        memchr::memmem::find(&self.0, needle)
    }

    /// Whether `needle` occurs anywhere.
    #[inline]
    pub fn contains_bytes(&self, needle: &[u8]) -> bool {
        self.find_bytes(needle).is_some()
    }

    /// `true` when every byte is ASCII — the case where byte semantics and
    /// character semantics cannot disagree, so callers can skip the locale
    /// question entirely.
    #[inline]
    pub fn is_ascii(&self) -> bool {
        self.0.is_ascii()
    }

    /// Characters, with `U+FFFD` standing in for any byte that is not part of
    /// valid UTF-8 — the character view for a multibyte locale.
    ///
    /// Named for what it does: a caller that wants the bytes must ask for
    /// [`Self::as_bytes`], because in a single-byte locale each byte is its own
    /// character and this iterator would merge some of them.
    #[inline]
    pub fn chars_lossy(&self) -> impl Iterator<Item = char> + '_ {
        self.0.utf8_chunks().flat_map(|c| {
            c.valid()
                .chars()
                .chain(std::iter::repeat_n('\u{fffd}', c.invalid().len()))
        })
    }

    /// Byte offset of the start of character `n` (0-based), or the end of the
    /// string when there are fewer than `n` characters.
    ///
    /// "Character" is a UTF-8 character where the bytes form one and a single
    /// byte otherwise, which is what makes character indexing total over
    /// arbitrary input: an unpaired byte is one character and stays itself.
    #[inline]
    pub fn char_offset(&self, n: usize) -> usize {
        let mut i = 0usize;
        let mut seen = 0usize;
        while seen < n && i < self.0.len() {
            i += crate::runtime::utf8_char_len(&self.0[i..]);
            seen += 1;
        }
        i
    }

    /// `count` characters starting at character `start`, as bytes.
    ///
    /// Cutting on character boundaries and copying the **bytes** between them is
    /// what keeps `substr` byte-faithful: rendering the characters back out
    /// would turn any unpaired byte into `U+FFFD`.
    #[inline]
    pub fn substr_chars(&self, start: usize, count: usize) -> AwkStr {
        let from = self.char_offset(start);
        let mut i = from;
        let mut taken = 0usize;
        while taken < count && i < self.0.len() {
            i += crate::runtime::utf8_char_len(&self.0[i..]);
            taken += 1;
        }
        AwkStr(self.0[from..i].to_vec())
    }

    /// `count` bytes starting at byte `start` — the `-b` counterpart of
    /// [`Self::substr_chars`].
    #[inline]
    pub fn substr_bytes(&self, start: usize, count: usize) -> AwkStr {
        if start >= self.0.len() {
            return AwkStr::new();
        }
        let end = start.saturating_add(count).min(self.0.len());
        AwkStr(self.0[start..end].to_vec())
    }

    /// Byte slice of a range, as a new `AwkStr`.
    #[inline]
    pub fn slice(&self, start: usize, end: usize) -> AwkStr {
        let s = start.min(self.0.len());
        let e = end.clamp(s, self.0.len());
        AwkStr(self.0[s..e].to_vec())
    }
}

impl Deref for AwkStr {
    type Target = [u8];
    #[inline]
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl From<String> for AwkStr {
    #[inline]
    fn from(s: String) -> Self {
        AwkStr(s.into_bytes())
    }
}

impl From<&String> for AwkStr {
    #[inline]
    fn from(s: &String) -> Self {
        AwkStr(s.as_bytes().to_vec())
    }
}

impl From<&str> for AwkStr {
    #[inline]
    fn from(s: &str) -> Self {
        AwkStr(s.as_bytes().to_vec())
    }
}

impl From<Vec<u8>> for AwkStr {
    #[inline]
    fn from(v: Vec<u8>) -> Self {
        AwkStr(v)
    }
}

impl From<&[u8]> for AwkStr {
    #[inline]
    fn from(b: &[u8]) -> Self {
        AwkStr(b.to_vec())
    }
}

impl From<Cow<'_, str>> for AwkStr {
    #[inline]
    fn from(c: Cow<'_, str>) -> Self {
        AwkStr(c.into_owned().into_bytes())
    }
}

impl From<char> for AwkStr {
    #[inline]
    fn from(c: char) -> Self {
        let mut s = AwkStr::new();
        s.push_char(c);
        s
    }
}

impl PartialEq<str> for AwkStr {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<&str> for AwkStr {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<String> for AwkStr {
    #[inline]
    fn eq(&self, other: &String) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<AwkStr> for str {
    #[inline]
    fn eq(&self, other: &AwkStr) -> bool {
        self.as_bytes() == other.0
    }
}

impl PartialEq<AwkStr> for &str {
    #[inline]
    fn eq(&self, other: &AwkStr) -> bool {
        self.as_bytes() == other.0
    }
}

impl PartialEq<[u8]> for AwkStr {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.0 == other
    }
}

/// Lossy, for user-facing text. The bytes are the value; this is a rendering.
impl fmt::Display for AwkStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_str_lossy(), f)
    }
}

/// Quoted like a string literal, with `\xNN` for bytes no `str` can hold, so a
/// `{:?}` of a `Value` still reads as text rather than a list of integers.
impl fmt::Debug for AwkStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_utf8() {
            Some(s) => fmt::Debug::fmt(s, f),
            None => {
                f.write_str("\"")?;
                for chunk in self.0.utf8_chunks() {
                    for c in chunk.valid().chars() {
                        write!(f, "{}", c.escape_debug())?;
                    }
                    for b in chunk.invalid() {
                        write!(f, "\\x{b:02x}")?;
                    }
                }
                f.write_str("\"")
            }
        }
    }
}

/// `write!(dst, ...)` appends UTF-8, which is always a valid byte sequence.
impl fmt::Write for AwkStr {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push_str(s);
        Ok(())
    }
}

impl std::borrow::Borrow<[u8]> for AwkStr {
    #[inline]
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl FromIterator<u8> for AwkStr {
    fn from_iter<I: IntoIterator<Item = u8>>(iter: I) -> Self {
        AwkStr(iter.into_iter().collect())
    }
}

impl FromIterator<char> for AwkStr {
    fn from_iter<I: IntoIterator<Item = char>>(iter: I) -> Self {
        let mut s = AwkStr::new();
        for c in iter {
            s.push_char(c);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_high_byte_survives_the_round_trip() {
        // The whole point: `String` cannot hold this, so every assertion here
        // is one the previous representation could not make.
        let s = AwkStr::from_vec(vec![b'a', 0xe9, b'b']);
        assert_eq!(s.as_bytes(), &[b'a', 0xe9, b'b']);
        assert_eq!(s.len(), 3, "length is bytes, not characters");
        assert!(s.as_utf8().is_none(), "not valid UTF-8, and not pretended");
        assert_eq!(s.to_str_lossy(), "a\u{fffd}b");
        assert_eq!(s.clone().into_bytes(), vec![b'a', 0xe9, b'b']);
    }

    #[test]
    fn valid_utf8_borrows_rather_than_copies() {
        let s = AwkStr::from("café");
        let borrowed = s.as_utf8().expect("valid UTF-8");
        assert_eq!(borrowed, "café");
        assert_eq!(s.len(), 5, "5 bytes, 4 characters");
        assert!(matches!(s.to_str_lossy(), Cow::Borrowed(_)));
    }

    #[test]
    fn equality_and_ordering_are_over_bytes() {
        assert_eq!(AwkStr::from("abc"), "abc");
        let (abc, abd) = (AwkStr::from("abc"), AwkStr::from("abd"));
        assert!(abc < abd);
        // 0xFF sorts above every ASCII byte, as `strcmp` has it.
        let (z, high) = (AwkStr::from("z"), AwkStr::from_vec(vec![0xff]));
        assert!(z < high);
    }

    #[test]
    fn debug_shows_text_with_escapes_for_unnameable_bytes() {
        assert_eq!(format!("{:?}", AwkStr::from("ab")), "\"ab\"");
        assert_eq!(
            format!("{:?}", AwkStr::from_vec(vec![b'a', 0xe9, b'b'])),
            "\"a\\xe9b\""
        );
    }

    #[test]
    fn find_bytes_locates_a_byte_no_str_can_name() {
        let s = AwkStr::from_vec(vec![b'a', 0xe9, b'b']);
        assert_eq!(s.find_bytes(&[0xe9]), Some(1));
        assert_eq!(s.find_bytes(b"b"), Some(2));
        assert_eq!(s.find_bytes(b"zz"), None);
        assert_eq!(s.find_bytes(b""), Some(0), "empty needle matches at 0");
    }

    #[test]
    fn slice_is_by_byte_offset_and_clamps() {
        let s = AwkStr::from_vec(vec![b'a', 0xe9, b'b']);
        assert_eq!(s.slice(1, 2).as_bytes(), &[0xe9]);
        assert_eq!(s.slice(0, 99).as_bytes(), &[b'a', 0xe9, b'b']);
        assert_eq!(s.slice(5, 9).as_bytes(), b"");
    }
}
