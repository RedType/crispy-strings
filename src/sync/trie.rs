use std::{collections::HashMap, hash::Hash, ops::Range};

pub(crate) type Span = Range<usize>;

pub(crate) fn span(start: usize, length: usize) -> Span {
  start..(start + length)
}

#[derive(Debug)]
pub(crate) struct Trie<T: Clone + Eq + Hash + Send + Sync> {
  span: Span,
  leaf_map: HashMap<T, Trie<T>>,
}

impl<T: Clone + Eq + Hash + Send + Sync> Trie<T> {
  pub(crate) fn new() -> Self {
    Self {
      span: span(0, 0),
      leaf_map: HashMap::new(),
    }
  }

  pub(crate) fn get<A: Iterator<Item = T> + Clone>(
    &mut self,
    mut key: A,
  ) -> Option<Span> {
    let mut cursor = self;

    loop {
      match key.next() {
        None => return Some(cursor.span.clone()),
        Some(k) => {
          cursor = cursor.leaf_map.get_mut(&k)?;
          continue;
        },
      }
    }
  }

  pub(crate) fn insert<A: Iterator<Item = T> + Clone>(
    &mut self,
    mut key: A,
    mut start: usize,
  ) -> Span {
    let span = self.insert_one(key.clone(), start);

    // insert all substrings
    loop {
      start += 1;
      if key.next().is_none() {
        break;
      }

      self.insert_one(key.clone(), start);
    }

    span
  }

  fn insert_one<A: Iterator<Item = T> + Clone>(
    &mut self,
    key: A,
    start: usize,
  ) -> Span {
    let mut cursor = self;
    let mut span_len = 0;

    for ref t in key {
      span_len += 1;

      // do not overwrite old entries
      if !cursor.leaf_map.contains_key(&t) {
        cursor.leaf_map.insert(
          t.clone(),
          Trie {
            span: span(start, span_len),
            leaf_map: HashMap::new(),
          },
        );
      }

      cursor = cursor.leaf_map.get_mut(&t).unwrap();
    }

    cursor.span.clone()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn trie() {
    let mut trie = Trie::new();

    let hello_span = trie.insert("hello".chars(), 0);

    assert_eq!(span(0, 5), hello_span);
    assert_eq!(Some(span(0, 4)), trie.get("hell".chars()));
    assert_eq!(Some(span(2, 2)), trie.get("ll".chars()));
    assert_eq!(None, trie.get("hohoho".chars()));

    let hoho_span = trie.insert("hoho".chars(), 3);

    assert_eq!(span(3, 4), hoho_span);
    assert_eq!(Some(span(0, 1)), trie.get("h".chars()));
    assert_eq!(Some(span(3, 2)), trie.get("ho".chars()));
    assert_eq!(Some(span(4, 2)), trie.get("oh".chars()));
    assert_eq!(Some(span(0, 2)), trie.get("he".chars()));
    assert_eq!(None, trie.get("hi".chars()));
  }
}
