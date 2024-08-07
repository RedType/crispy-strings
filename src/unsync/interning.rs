use crate::unsync::{
  error::InternError,
  trie::{Span, Trie},
};
use std::{
  cell::{Cell, UnsafeCell},
  fmt,
  ops::Deref,
  panic::Location,
  rc::Rc,
};

#[derive(Debug)]
pub struct Interner {
  index: UnsafeCell<Trie<char>>,
  store: UnsafeCell<String>,
  refs: Cell<usize>,
  last_ref: Cell<Option<&'static Location<'static>>>,
}

impl Interner {
  pub fn new() -> Rc<Self> {
    Rc::new(Self {
      index: UnsafeCell::new(Trie::new()),
      store: UnsafeCell::new(String::new()),
      refs: Cell::new(0),
      last_ref: Cell::new(None),
    })
  }

  pub fn extract_store(self) -> String {
    self.store.into_inner()
  }

  pub fn intern<S: AsRef<str>>(self: &Rc<Self>, s: S) -> Intern {
    match self.try_intern(s) {
      Ok(intern) => intern,
      Err(InternError::OutstandingRef(loc)) => {
        panic!(
          concat!(
            "Cannot intern while holding an InternRef\n",
            " --> last InternRef created at {}:{}:{}",
          ),
          loc.file(),
          loc.line(),
          loc.column()
        );
      },
    }
  }

  // SAFETY: We do not allow mutation of the store if there
  //         are existing InternRefs
  pub fn try_intern<S: AsRef<str>>(
    self: &Rc<Self>,
    s: S,
  ) -> Result<Intern, InternError> {
    if self.refs.get() > 0 {
      let lr = self.last_ref.get().unwrap();
      return Err(InternError::OutstandingRef(lr));
    }

    let index = unsafe { &mut *self.index.get() };
    let store = unsafe { &mut *self.store.get() };
    let span = index.get(s.as_ref().chars()).unwrap_or_else(|| {
      let start = store.len();
      store.push_str(s.as_ref());
      index.insert(s.as_ref().chars(), start)
    });

    Ok(Intern {
      span,
      interner: Rc::clone(self),
    })
  }
}

#[derive(Clone)]
pub struct Intern {
  span: Span,
  interner: Rc<Interner>,
}

impl Intern {
  // SAFETY: we disallow store modification while InternRefs exist
  #[track_caller]
  pub fn get_ref(&self) -> InternRef<'_> {
    // register last caller location for helpful panics
    #[cfg(debug_assertions)]
    self.interner.last_ref.set(Some(Location::caller()));

    // increment refcount
    self.interner.refs.set(self.interner.refs.get() + 1);

    let store = unsafe { &*self.interner.store.get() };
    let text = &store[self.span.clone()];
    InternRef {
      text,
      interner: Rc::clone(&self.interner),
    }
  }
}

impl fmt::Debug for Intern {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Intern")
      .field("span", &self.span)
      .field("text", &&*self.get_ref())
      .finish()
  }
}

impl fmt::Display for Intern {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&*self.get_ref())
  }
}

impl PartialEq for Intern {
  fn eq(&self, other: &Self) -> bool {
    Rc::ptr_eq(&self.interner, &other.interner) && self.span == other.span
  }
}
impl Eq for Intern {}

pub struct InternRef<'a> {
  text: &'a str,
  interner: Rc<Interner>,
}

impl fmt::Debug for InternRef<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_tuple("InternRef").field(&self.text).finish()
  }
}

impl fmt::Display for InternRef<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.text)
  }
}

impl Deref for InternRef<'_> {
  type Target = str;

  fn deref(&self) -> &Self::Target {
    self.text
  }
}

impl Drop for InternRef<'_> {
  fn drop(&mut self) {
    if self.interner.refs.get() == 0 {
      panic!("Interner reference count overflow");
    } else {
      self.interner.refs.set(self.interner.refs.get() - 1);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::ptr;

  #[test]
  fn interner() {
    let interner = Interner::new();
    let hello1 = interner.intern("hello");
    let goodbye = interner.intern("goodbye");
    let hello2 = interner.intern("hello");

    assert_eq!(&*hello1.get_ref(), "hello");
    assert_eq!(&*hello2.get_ref(), "hello");
    assert_eq!(&*goodbye.get_ref(), "goodbye");

    assert_eq!(*hello1.get_ref(), *hello2.get_ref());
    assert_ne!(*hello1.get_ref(), *goodbye.get_ref());

    assert!(ptr::addr_eq(&*hello1.get_ref(), &*hello2.get_ref()));
    assert!(!ptr::addr_eq(&*hello1.get_ref(), &*goodbye.get_ref()));
  }

  #[test]
  #[should_panic]
  fn interner_errors_while_holding_refs() {
    let interner = Interner::new();
    let hello = interner.intern("hello");
    let _hello_ref = hello.get_ref();
    interner.intern("hello"); // this should panic
  }
}
